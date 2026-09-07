use axum::{
    Json, Router,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub struct Login {
    username: String,
    password: String,
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

    (
        StatusCode::OK,
        Json(json!({"error": "Login is successful!"})),
    )
        .into_response()
}

pub async fn new() -> Router {
    let router: Router = Router::new()
        .route("/", get(home))
        .route("/login", post(login));
    router
}
