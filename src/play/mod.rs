pub mod page;

use axum::{
    Router,
    http::header,
    response::{Html, IntoResponse},
    routing::get,
};

use crate::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/play", get(page::play))
        .route("/assets/play.css", get(stylesheet))
        .route("/assets/play.js", get(javascript))
        .route(
            "/assets/models/command-center.stl",
            get(command_center_stl),
        )
}

async fn stylesheet() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("play.css"),
    )
}

async fn javascript() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("play.js"),
    )
}

async fn command_center_stl() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "model/stl"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        include_bytes!("../web/usa/command-center.stl").as_slice(),
    )
}

pub async fn index() -> Html<&'static str> {
    Html(include_str!("index.html"))
}
