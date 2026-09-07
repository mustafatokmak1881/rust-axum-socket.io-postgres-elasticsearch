use axum::{
    Json, Router,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use jsonwebtoken::{EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;

#[derive(Deserialize)]
pub struct Login {
    username: String,
    password: String,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Claims {
    sub: String,
    exp: usize,
}

pub async fn home() -> &'static str {
    "Auth Home Page"
}

pub async fn login(Json(payload): Json<Login>) -> impl IntoResponse {
    let login_success: bool = &payload.username == "admin" && &payload.password == "123456";

    if !login_success {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Wrong username or password !"})),
        )
            .into_response();
    }

    let secret = env::var("JWT_TOKEN").expect("JWT_TOKEN not set in the .env file!");

    let expiration = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::hours(24))
        .expect("Invalid time calculation !")
        .timestamp() as usize;

    let claims = Claims {
        sub: payload.username,
        exp: expiration,
    };

    let token = match encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    ) {
        Ok(t) => t,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Token creation failed"})),
            )
                .into_response();
        }
    };

    (
        StatusCode::OK,
        Json(json!({"message": "Login is successful!", "token": token})),
    )
        .into_response()
}

pub async fn new() -> Router {
    let router: Router = Router::new()
        .route("/", get(home))
        .route("/login", post(login));
    router
}
