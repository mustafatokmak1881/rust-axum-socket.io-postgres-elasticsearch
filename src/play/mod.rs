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
        .route("/assets/models/barracks.stl", get(barracks_stl))
        .route("/assets/terrain.jpg", get(terrain_jpg))
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

async fn barracks_stl() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "model/stl"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        include_bytes!("../web/usa/barracks.stl").as_slice(),
    )
}

async fn terrain_jpg() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "image/jpeg"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        include_bytes!("../web/village-terrain.jpg").as_slice(),
    )
}

pub async fn index() -> Html<&'static str> {
    Html(include_str!("index.html"))
}
