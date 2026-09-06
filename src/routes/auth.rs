use axum::{Json, debug_handler};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Auth {
    username: String,
    password: String,
}

impl Auth {
    #[debug_handler]
    pub async fn home() -> &'static str {
        "Home Page"
    }

    #[debug_handler]
    pub async fn login(Json(payload): Json<Auth>) -> &'static str {
        println!(
            "default username, password: {}:{}",
            payload.username, payload.password
        );

        "Login Page"
    }
}
