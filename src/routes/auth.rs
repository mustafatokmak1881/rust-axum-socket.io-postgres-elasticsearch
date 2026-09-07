use axum::{
    Json, Router, middleware,
    routing::{get, post},
};
use serde::Deserialize;

use crate::middlewares::middlewares::is_authenticated;

#[derive(Deserialize)]
pub struct Login {
    username: String,
    password: String,
}

pub async fn home() -> &'static str {
    "Login Home Page"
}

pub async fn login(Json(payload): Json<Login>) -> &'static str {
    println!("Credentials: {}:{}", &payload.username, &payload.password);

    "Login Page"
}

pub async fn new() -> Router {
    let router: Router = Router::new()
        .route("/", get(home))
        .route("/login", post(login))
        .route_layer(middleware::from_fn(is_authenticated));

    router
}
