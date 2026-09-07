use crate::middlewares::middlewares::is_authenticated;
use axum::{Router, middleware, routing::get};

pub async fn home() -> &'static str {
    "Restricted User Home"
}

pub async fn new() -> Router {
    let router: Router = Router::new()
        .route("/", get(home))
        .route_layer(middleware::from_fn(is_authenticated)); // Restricted - Required Success Login

    router
}
