use axum::{
    Json, Router,
    routing::{get, post},
};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Login {
    username: String,
    password: String,
}

pub async fn home() -> &'static str {
    "Auth Home Page"
}

pub async fn login(Json(payload): Json<Login>) -> &'static str {
    println!("Credentials: {}:{}", &payload.username, &payload.password);

    "Auth Login Page"
}

pub async fn new() -> Router {
    let router: Router = Router::new()
        .route("/", get(home))
        .route("/login", post(login));
    router
}
