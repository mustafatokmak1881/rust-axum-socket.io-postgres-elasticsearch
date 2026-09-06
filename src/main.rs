use axum::{Json, Router, routing::get, routing::post};
use dotenvy::dotenv;
use serde_json::Value;
use std::env;
use tokio::net::TcpListener;

async fn home_route() -> &'static str {
    "Home Endpoint"
}

async fn login_route(Json(body): Json<Value>) -> &'static str {
    println!("Username: {}", body["username"]);
    "Login Endpoint"
}

#[tokio::main]
async fn main() {
    dotenv().ok(); // Dotenv install

    let host: String = env::var("HOST").expect("HOST is not set in the .env file!");
    let port: String = env::var("PORT").expect("PORT is not set in the .env file!");

    let full_url: String = format!("{}:{}", &host, &port);
    let listener: TcpListener = TcpListener::bind(&full_url).await.unwrap();

    let app = Router::new()
        .route("/", get(home_route))
        .route("/auth/login", post(login_route));

    println!("Full_url:{}", &full_url);
    axum::serve(listener, app).await.unwrap();
}
