use crate::routes::auth::Claims;
use axum::{
    Json,
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use jsonwebtoken::{DecodingKey, Validation, decode};
use serde_json::json;
use std::env;

pub async fn logger_middleware(req: Request, next: Next) -> Response {
    println!("Url: {:?}", req.uri());
    let response = next.run(req).await;

    response
}

pub async fn is_authenticated(req: Request, next: Next) -> Response {
    let auth_header = match req.headers().get("authorization") {
        Some(auth_header) => auth_header,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Authorization header is missing"})),
            )
                .into_response();
        }
    };

    let auth_str: &str = match auth_header.to_str() {
        Ok(s) => s,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Authorization header is missing"})),
            )
                .into_response();
        }
    };

    let token: &str = auth_str
        .strip_prefix("bearer ")
        .or_else(|| auth_str.strip_prefix("Bearer "))
        .unwrap_or(auth_str)
        .trim();

    let secret = env::var("JWT_TOKEN").expect("JWT_TOKEN not set in the .env file!");

    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    );

    match token_data {
        Ok(_) => {
            // Türü sildik, sadece 'data' bıraktık
            println!("Token valid");
        }
        Err(err) => {
            // Türü sildik, sadece 'err' bıraktık
            println!("Invalid token: {:?}", err);

            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Invalid token"})),
            )
                .into_response();
        }
    }

    let response = next.run(req).await;

    response
}
