use std::net::{SocketAddr, TcpListener};
use axum::{routing::get, Router};
use redis::Commands;
use system::dmesg;

async fn access_internet() -> String {
    let url = "https://jsonplaceholder.typicode.com/todos/1";
    let response = reqwest::get(url).await.unwrap().text().await.unwrap();

    response
}

async fn connect_redis() -> String {
    let client = redis::Client::open("redis://127.0.0.1:6379").unwrap();
    let mut con = client.get_connection().unwrap();
    let _: () = con.set("my_key", 42).unwrap();
    
    let val: String = con.get("my_key").unwrap();
    val
    // "ok".to_string()
}
// redis server/
// policy engine server microservice enclave => signer engine

pub async fn start_server() {
    // Build our application with routes
    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route("/redis", get(connect_redis))
        .route("/access-internet", get(access_internet));

    // Define the address to bind to
    let addr = "127.0.0.1:8000".parse::<SocketAddr>().expect("Invalid address");

    // Try binding the TcpListener
    let listener = match TcpListener::bind(addr) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("Failed to bind to address: {}", e);
            return;
        }
    };

    // Log successful start
    println!("Server started on {}", addr);

    // Start the server
    if let Err(e) = axum::Server::from_tcp(listener).unwrap().serve(app.into_make_service()).await {
        eprintln!("Server error: {}", e);
    }
}

// #[tokio::main]
// pub async fn run_server() {
//     start_server().await;
// }
