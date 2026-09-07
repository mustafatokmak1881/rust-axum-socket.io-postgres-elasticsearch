use axum::{Json, Router, routing::get};
use serde::Deserialize;

// #[derive(Deserialize)]
// pub struct Auth {
//     username: String,
//     password: String,
// }

// impl Auth {
//     pub fn new() {
//         let user_routes: Router = Router::new().route("/loginx", get(|| async { "Loginx Page" }));
//     }
//     pub async fn home() -> &'static str {
//         "Home Page"
//     }

//     pub async fn login(Json(payload): Json<Auth>) -> &'static str {
//         println!(
//             "username, password: {}:{}",
//             payload.username, payload.password
//         );

//         "Login Page"
//     }
// }

pub async fn home() -> &'static str {
    "Login Page"
}

pub async fn router() -> Router {
    let routes: Router = Router::new().route("/", get(home));

    routes
}
