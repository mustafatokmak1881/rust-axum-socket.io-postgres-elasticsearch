use axum::{
    Json,
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;

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

    println!("Auth_str: {}", token);

    let response = next.run(req).await;

    response
}
