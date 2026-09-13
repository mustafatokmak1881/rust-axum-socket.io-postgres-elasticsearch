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
        .route("/assets/models/command-center.obj", get(command_center_obj))
        .route("/assets/models/command-center.mtl", get(command_center_mtl))
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

async fn command_center_obj() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        include_bytes!("../web/usa/command-center.obj").as_slice(),
    )
}

async fn command_center_mtl() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        include_bytes!("../web/usa/command-center.mtl").as_slice(),
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
