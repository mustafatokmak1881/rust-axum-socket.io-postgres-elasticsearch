use axum::Json;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Auth {
    username: String,
    password: String,
}

impl Auth {
    pub async fn home() -> &'static str {
        "Home Page"
    }

    pub async fn login(Json(payload): Json<Auth>) -> &'static str {
        println!("username: {}", payload.username);
        "Login Page"
    }
}
