use server::start_server;
use std::io::{self, Write};
use std::thread;
use std::{
    fs,
    io::Read,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
};
use system::{dmesg, freopen, mount, seed_entropy};

//TODO: Feature flag
use aws::{get_entropy, init_platform};

// Mount common filesystems with conservative permissions
fn init_rootfs() {
    use libc::{MS_NODEV, MS_NOEXEC, MS_NOSUID};
    let no_dse = MS_NODEV | MS_NOSUID | MS_NOEXEC;
    let no_se = MS_NOSUID | MS_NOEXEC;
    let args = [
        ("devtmpfs", "/dev", "devtmpfs", no_se, "mode=0755"),
        ("devtmpfs", "/dev", "devtmpfs", no_se, "mode=0755"),
        ("devpts", "/dev/pts", "devpts", no_se, ""),
        ("shm", "/dev/shm", "tmpfs", no_dse, "mode=0755"),
        ("proc", "/proc", "proc", no_dse, "hidepid=2"),
        ("tmpfs", "/run", "tmpfs", no_dse, "mode=0755"),
        ("tmpfs", "/tmp", "tmpfs", no_dse, ""),
        ("sysfs", "/sys", "sysfs", no_dse, ""),
        (
            "cgroup_root",
            "/sys/fs/cgroup",
            "tmpfs",
            no_dse,
            "mode=0755",
        ),
    ];
    for (src, target, fstype, flags, data) in args {
        match mount(src, target, fstype, flags, data) {
            Ok(()) => dmesg(format!("Mounted {}", target)),
            Err(e) => eprintln!("{}", e),
        }
    }
}

// Initialize console with stdin/stdout/stderr
fn init_console() {
    let args = [
        ("/dev/console", "r", 0),
        ("/dev/console", "w", 1),
        ("/dev/console", "w", 2),
    ];
    for (filename, mode, file) in args {
        match freopen(filename, mode, file) {
            Ok(()) => {}
            Err(e) => eprintln!("{}", e),
        }
    }
}

// Pipe streams are blocking, we need separate threads to monitor them without blocking the primary thread.
fn child_stream_to_vec<R>(mut stream: R) -> Arc<Mutex<Vec<u8>>>
where
    R: Read + Send + 'static,
{
    let out = Arc::new(Mutex::new(Vec::new()));
    let vec = out.clone();
    thread::Builder::new()
        .name("child_stream_to_vec".into())
        .spawn(move || loop {
            let mut buf = [0];
            match stream.read(&mut buf) {
                Err(err) => {
                    println!("{}] Error reading from stream: {}", line!(), err);
                    break;
                }
                Ok(got) => {
                    if got == 0 {
                        break;
                    } else if got == 1 {
                        vec.lock().expect("!lock").push(buf[0])
                    } else {
                        println!("{}] Unexpected number of bytes: {}", line!(), got);
                        break;
                    }
                }
            }
        })
        .expect("!thread");
    out
}

fn debug_filesystem() {
    dmesg("Debugging filesystem:".to_string());

    // List root directory
    match fs::read_dir("/") {
        Ok(entries) => {
            for entry in entries {
                if let Ok(entry) = entry {
                    dmesg(format!("Found in /: {:?}", entry.path()));
                }
            }
        }
        Err(e) => dmesg(format!("Error reading /: {}", e)),
    }

    // Check vm file
    match fs::metadata("/vm") {
        Ok(metadata) => {
            dmesg(format!("vm metadata: {:?}", metadata));
            dmesg(format!(
                "vm permissions: {:o}",
                metadata.permissions().mode()
            ));
        }
        Err(e) => dmesg(format!("Error getting vm metadata: {}", e)),
    }

    // Try to execute socat with --version
    match Command::new("/vm").arg("-dkk").output() {
        Ok(output) => {
            dmesg(format!(
                "socat -h output: {:?}",
                String::from_utf8_lossy(&output.stdout)
            ));
            dmesg(format!(
                "socat -h error: {:?}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Err(e) => dmesg(format!("Error executing vm -dkk: {}", e)),
    }
}

fn start_socat_redirection() {
    debug_filesystem();

    let ifconfig = Command::new("/ifconfig")
        .args(&["lo", "127.0.0.1"])
        .output()
        .expect("failed to execute process");

    println!("status: {}", ifconfig.status);
    io::stdout().write_all(&ifconfig.stdout).unwrap();
    io::stderr().write_all(&ifconfig.stderr).unwrap();

    let mut vm = Command::new("/vm")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("!vm");
    let out = child_stream_to_vec(vm.stdout.take().expect("!stdout"));
    let err = child_stream_to_vec(vm.stderr.take().expect("!stderr"));
    let mut stdin = match vm.stdin.take() {
        Some(stdin) => stdin,
        None => panic!("!stdin"),
    };
}

// match Command::new("/socat")
//     .args(&[
//         "-d",
//         "-d",
//         "-t",
//         "30",
//         "VSOCK-LISTEN:1000,fork,reuseaddr",
//         "TCP:127.0.0.1:8000",
//     ])
//     .spawn()
// {
//     Ok(output) => dmesg(format!("Started socat redirection: {:?}", output)),
//     Err(e) => dmesg(format!("Failed to start socat: {}", e)),
// }
// }

fn boot() {
    init_rootfs();
    init_console();
    init_platform();
    match seed_entropy(4096, get_entropy) {
        Ok(size) => dmesg(format!("Seeded kernel with entropy: {}", size)),
        Err(e) => eprintln!("{}", e),
    };
    // Start socat redirection
    start_socat_redirection();
}

#[tokio::main]
async fn main() {
    boot();
    dmesg("EnclaveOS Booted".to_string());
    start_server().await;
}

// inside enclave socat connection
// /usr/bin/socat -t 30 VSOCK-LISTEN:1000,fork,reuseaddr TCP:127.0.0.1:8000 &

// outside enclave socat connection
// socat -t 30 TCP-LISTEN:80,fork,reuseaddr VSOCK-CONNECT:7777:1000 &
