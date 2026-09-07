mod middlewares;
mod routes;

use axum::{Router, middleware, routing::get, routing::post};
use dotenvy::dotenv;
use middlewares::middlewares::logger_middleware;
use routes::auth::Auth;
use std::env;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    dotenv().ok();

    let host: String = env::var("HOST").expect("HOST is not set in the .env file!");
    let port: String = env::var("PORT").expect("PORT is not set in the .env file!");

    let full_url: String = format!("{}:{}", &host, &port);
    let listener: TcpListener = TcpListener::bind(&full_url).await.unwrap();

    // KRİTİK DÜZELTME: Fonksiyonların sonundaki parantezleri () sildik.
    let app = Router::new()
        .route("/", get(Auth::home))
        .route("/auth/login", post(Auth::login))
        .layer(middleware::from_fn(logger_middleware));

    println!("Full_url: {}", &full_url);
    axum::serve(listener, app).await.unwrap();
}
