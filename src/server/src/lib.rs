use axum::{
    routing::get,
    Router,
};
use system::dmesg;

pub async fn start_server() {
    // build our application with a single route
    let app = Router::new().route("/", get(|| async { "Hello, World!" }));

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
    dmesg("server started!!".to_string());
}

// #[tokio::main]
// pub async fn run_server() {
//     start_server().await;
// }
