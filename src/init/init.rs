use libc::{fcntl, F_GETFL, F_SETFL, O_NONBLOCK};
use polling::{Event, Events, Poller};
use server::start_server;
use std::fs::OpenOptions;
use std::io::{self, BufRead, BufReader, Write};
use std::num::NonZero;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
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
use tokio::task;

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

fn set_nonblocking<H>(handle: &H, nonblocking: bool) -> std::io::Result<()>
where
    H: Read + AsRawFd,
{
    let fd = handle.as_raw_fd();
    let flags = unsafe { fcntl(fd, F_GETFL, 0) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let flags = if nonblocking {
        flags | O_NONBLOCK
    } else {
        flags & !O_NONBLOCK
    };
    let res = unsafe { fcntl(fd, F_SETFL, flags) };
    if res != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
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
}

fn start_redirection() -> Result<(), std::io::Error> {

    let path = PathBuf::from("/vm").canonicalize()?;

    let mut child = Command::new(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped()) // Pipe stdout
        .stderr(Stdio::piped()) // Pipe stderr
        .spawn()
        .expect("Failed to start worker");

    let stdout = child.stdout.take().unwrap();
    set_nonblocking(&stdout, true)?;
    let stderr = child.stderr.take().unwrap();
    set_nonblocking(&stderr, true)?;

    // Poll for both stdout and stderr
    let handle = thread::spawn(move || {
        let mut reader_out = BufReader::new(stdout);
        let mut reader_err = BufReader::new(stderr);
        let poller = Poller::new().unwrap();
        let key_out = 1;
        let key_err = 2;
        let mut out_closed = false;
        let mut err_closed = false;
        let mut line = String::new();
        let mut events = Events::with_capacity(NonZero::new(2).unwrap());

        unsafe {
            poller
                .add(reader_out.get_ref(), Event::readable(key_out))
                .unwrap();
            poller
                .add(reader_err.get_ref(), Event::readable(key_err))
                .unwrap();
        }

        loop {
            events.clear();
            poller.wait(&mut events, None).unwrap();

            for ev in events.iter() {
                if ev.key == key_out {
                    let len = match reader_out.read_line(&mut line) {
                        Ok(len) => len,
                        Err(e) => {
                            println!("stdout error: {}", e);
                            0
                        }
                    };
                    if len == 0 {
                        out_closed = true;
                        poller.delete(reader_out.get_ref()).unwrap();
                    } else {
                        print!("[STDOUT] {}", line);
                        line.clear();
                        poller
                            .modify(reader_out.get_ref(), Event::readable(key_out))
                            .unwrap();
                    }
                }
                if ev.key == key_err {
                    let len = match reader_err.read_line(&mut line) {
                        Ok(len) => len,
                        Err(e) => {
                            println!("stderr error: {}", e);
                            0
                        }
                    };
                    if len == 0 {
                        err_closed = true;
                        poller.delete(reader_err.get_ref()).unwrap();
                    } else {
                        print!("[STDERR] {}", line);
                        line.clear();
                        poller
                            .modify(reader_err.get_ref(), Event::readable(key_err))
                            .unwrap();
                    }
                }
            }

            if out_closed && err_closed {
                println!("Stream closed, exiting process thread");
                break;
            }
        }
    });

    handle.join().unwrap();
    Ok(())
}


fn reverse_proxy() -> Result<(), std::io::Error> {

    let path = PathBuf::from("/caddy").canonicalize()?;

    let mut child = Command::new(path)
        .arg("run")
        .arg("--config")
        .arg("/Caddyfile")
        .stdin(Stdio::null())
        .stdout(Stdio::piped()) // Pipe stdout
        .stderr(Stdio::piped()) // Pipe stderr
        .spawn()
        .expect("Failed to start worker");

    let stdout = child.stdout.take().unwrap();
    set_nonblocking(&stdout, true)?;
    let stderr = child.stderr.take().unwrap();
    set_nonblocking(&stderr, true)?;

    // Poll for both stdout and stderr
    let handle = thread::spawn(move || {
        let mut reader_out = BufReader::new(stdout);
        let mut reader_err = BufReader::new(stderr);
        let poller = Poller::new().unwrap();
        let key_out = 1;
        let key_err = 2;
        let mut out_closed = false;
        let mut err_closed = false;
        let mut line = String::new();
        let mut events = Events::with_capacity(NonZero::new(2).unwrap());

        unsafe {
            poller
                .add(reader_out.get_ref(), Event::readable(key_out))
                .unwrap();
            poller
                .add(reader_err.get_ref(), Event::readable(key_err))
                .unwrap();
        }

        loop {
            events.clear();
            poller.wait(&mut events, None).unwrap();

            for ev in events.iter() {
                if ev.key == key_out {
                    let len = match reader_out.read_line(&mut line) {
                        Ok(len) => len,
                        Err(e) => {
                            println!("stdout error: {}", e);
                            0
                        }
                    };
                    if len == 0 {
                        out_closed = true;
                        poller.delete(reader_out.get_ref()).unwrap();
                    } else {
                        print!("[STDOUT] {}", line);
                        line.clear();
                        poller
                            .modify(reader_out.get_ref(), Event::readable(key_out))
                            .unwrap();
                    }
                }
                if ev.key == key_err {
                    let len = match reader_err.read_line(&mut line) {
                        Ok(len) => len,
                        Err(e) => {
                            println!("stderr error: {}", e);
                            0
                        }
                    };
                    if len == 0 {
                        err_closed = true;
                        poller.delete(reader_err.get_ref()).unwrap();
                    } else {
                        print!("[STDERR] {}", line);
                        line.clear();
                        poller
                            .modify(reader_err.get_ref(), Event::readable(key_err))
                            .unwrap();
                    }
                }
            }

            if out_closed && err_closed {
                println!("Stream closed, exiting process thread");
                break;
            }
        }
    });

    handle.join().unwrap();
    Ok(())
}

fn boot() {
    init_rootfs();
    init_console();
    init_platform();
    match seed_entropy(4096, get_entropy) {
        Ok(size) => dmesg(format!("Seeded kernel with entropy: {}", size)),
        Err(e) => eprintln!("{}", e),
    };
}

// fn configure_dns() -> io::Result<()> {
//     // Path to the resolv.conf file inside the VM
//     let resolv_conf_path = "/etc/resolv.conf";
    
//     // Open the file in write mode, truncating the previous contents
//     let mut file = OpenOptions::new()
//         .write(true)
//         .create(true)
//         .truncate(true)
//         .open(resolv_conf_path)?;
    
//     // DNS configuration to write
//     let dns_config = "nameserver 192.168.127.1\n";
    
//     // Write the DNS configuration to the file
//     file.write_all(dns_config.as_bytes())?;
    
//     // Ensure everything is written to the file
//     file.flush()?;
    
//     Ok(())
// }

#[tokio::main]
async fn main() {
    boot();
    debug_filesystem();
    dmesg("EnclaveOS Booted".to_string());
    
    // match configure_dns() {
    //     Ok(_) => println!("DNS configuration updated successfully."),
    //     Err(e) => eprintln!("Failed to update DNS configuration: {}", e),
    // }

    // Spawn a task to handle the redirection so that it doesn't block the server
    let redirection_task = tokio::task::spawn_blocking(|| {
        start_redirection().unwrap();
    });
    // let reverse_proxy =  tokio::task::spawn_blocking(|| {
    //     reverse_proxy().unwrap();
    // }); 
    // Start the server asynchronously
    let server_task = tokio::spawn(async {
        start_server().await;
    });

    let test_server = tokio::spawn(async {
        let url = "http://192.168.127.2:8000";
        print!("testing server now!");
        match reqwest::get(url).await {
            Ok(response) => match response.text().await {
                Ok(text) => {
                    println!("{}", text);
                }
                Err(e) => {
                    eprintln!("Failed to read response text: {}", e);
                }
            },
            Err(e) => {
                eprintln!("Failed to make GET request: {}", e);
            }
        }

        match reqwest::get(format!("{}/access-internet", url)).await {
            Ok(response) => match response.text().await {
                Ok(text) => {
                    println!("{}", text);
                }
                Err(e) => {
                    eprintln!("Failed to read response text: {}", e);
                }
            },
            Err(e) => {
                eprintln!("Failed to make GET request: {}", e);
            }
        }
        match reqwest::get(format!("{}/redis", url)).await {
            Ok(response) => match response.text().await {
                Ok(text) => {
                    println!("{}", text);
                }
                Err(e) => {
                    eprintln!("Failed to read response text: {}", e);
                }
            },
            Err(e) => {
                eprintln!("Failed to make GET request: {}", e);
            }
        }
    });

    // Wait for both tasks to complete
    tokio::join!(redirection_task, server_task, test_server);
}

// inside enclave socat connection
// /usr/bin/socat -t 30 VSOCK-LISTEN:1000,fork,reuseaddr TCP:127.0.0.1:8000 &

// outside enclave socat connection
// socat -t 30 TCP-LISTEN:80,fork,reuseaddr VSOCK-CONNECT:7777:1000 &
