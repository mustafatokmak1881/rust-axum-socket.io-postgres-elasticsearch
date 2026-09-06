mod routes;

use axum::{Router, routing::get, routing::post};
use dotenvy::dotenv;
use std::env;
use tokio::net::TcpListener;

// routes klasörünün içindeki auth modülünden Auth struct'ını çekiyoruz
use routes::auth::Auth;

#[tokio::main]
async fn main() {
    dotenv().ok();

    let host: String = env::var("HOST").expect("HOST is not set in the .env file!");
    let port: String = env::var("PORT").expect("PORT is not set in the .env file!");

    let full_url: String = format!("{}:{}", &host, &port);
    let listener: TcpListener = TcpListener::bind(&full_url).await.unwrap();

    // KRİTİK DÜZELTME: Fonksiyonların sonundaki parantezleri () sildik.
    let app = Router::new()
        .route("/", get(Auth::home ))
        .route("/auth/login", post( Auth::login ));

    println!("Full_url: {}", &full_url);
    axum::serve(listener, app).await.unwrap();
}
