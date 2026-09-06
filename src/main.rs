use axum::{Router, routing::get};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    let app: Router = Router::new().route("/", get(|| async { "Home Page" }));
    let listener: TcpListener = TcpListener::bind("0.0.0.0:3000")
        .await
        .unwrap();

    axum::serve(listener, app).await.unwrap();

    println!("Server lsitening");
}
