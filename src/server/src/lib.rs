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
    // build our application with a single route
    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route("/redis", get(connect_redis))
        .route("/access-internet", get(access_internet));
    // run our app with hyper, listening globally on port 8000
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
    dmesg("server started!!".to_string());
}

// #[tokio::main]
// pub async fn run_server() {
//     start_server().await;
// }
