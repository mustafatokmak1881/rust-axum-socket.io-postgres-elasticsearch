mod middlewares;
mod routes;

use axum::{Router, middleware};
use dotenvy::dotenv;
use middlewares::middlewares::logger_middleware;
use std::env;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    dotenv().ok();

    let host: String = env::var("HOST").expect("HOST is not set in the .env file!");
    let port: String = env::var("PORT").expect("PORT is not set in the .env file!");

    let full_url: String = format!("{}:{}", &host, &port);
    let listener: TcpListener = TcpListener::bind(&full_url).await.unwrap();

    let auth_routes: Router = routes::auth::new().await;

    // KRİTİK DÜZELTME: Fonksiyonların sonundaki parantezleri () sildik.
    let app = Router::new()
        .nest("/auth", auth_routes)
        .layer(middleware::from_fn(logger_middleware));

    println!("Full_url: {}", &full_url);
    axum::serve(listener, app).await.unwrap();
}
