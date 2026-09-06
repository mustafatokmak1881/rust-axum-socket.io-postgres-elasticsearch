use axum::{Router, routing::get};
use dotenvy::dotenv;
use std::env;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    dotenv().ok(); // Dotenv install

    let host: String = env::var("HOST").expect("HOST is not set in the .env file!");
    let port: String = env::var("PORT").expect("PORT is not set in the .env file!");

    let full_url: String = format!("{}:{}", &host, &port);

    let app: Router = Router::new().route("/", get(|| async { "Home Page" }));
    let listener: TcpListener = TcpListener::bind(&full_url).await.unwrap();

    println!("Full_url:{}", &full_url);
    axum::serve(listener, app).await.unwrap();
}
