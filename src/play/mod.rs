pub mod page;

use axum::{
    Router,
    body::Body,
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::get,
};

use crate::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/play", get(page::play))
        .route("/favicon.ico", get(favicon))
        .route("/assets/play.css", get(stylesheet))
        .route("/assets/play.js", get(javascript))
        .route("/assets/models/command-center.obj", get(command_center_obj))
        .route("/assets/models/command-center.mtl", get(command_center_mtl))
        .route("/assets/models/war-factory.obj", get(war_factory_obj))
        .route("/assets/models/war-factory.mtl", get(war_factory_mtl))
        .route("/assets/models/barracks.stl", get(barracks_stl))
        .route("/assets/terrain.jpg", get(terrain_jpg))
        .route("/assets/sounds/tank-move.mp3", get(tank_move_mp3))
        .route("/assets/sounds/tank-shoot.wav", get(tank_shoot_wav))
        .route("/assets/sounds/tank-destroyed.mp3", get(tank_destroyed_mp3))
        .route("/assets/sounds/mlrs-rocket.mp3", get(mlrs_rocket_mp3))
        .route("/assets/sounds/soldier-shoot.mp3", get(soldier_shoot_mp3))
        .route("/assets/sounds/building.mp3", get(building_mp3))
}

async fn favicon() -> Response {
    // Use r## so fill="#..." does not terminate the raw string early.
    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><rect width="32" height="32" rx="6" fill="#1a2614"/><path d="M8 22V10h4l4 8 4-8h4v12h-3.2V14.5L16.5 22h-1L11.2 14.5V22z" fill="#7cfc00"/></svg>"##;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/svg+xml")
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .body(Body::from(SVG))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
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

async fn war_factory_obj() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        include_bytes!("../web/usa/war-factory.obj").as_slice(),
    )
}

async fn war_factory_mtl() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        include_bytes!("../web/usa/war-factory.mtl").as_slice(),
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

async fn tank_move_mp3() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "audio/mpeg"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        include_bytes!("../web/usa/sounds/tank-move.mp3").as_slice(),
    )
}

async fn tank_shoot_wav() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "audio/wav"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        include_bytes!("../web/usa/sounds/tank-shoot.wav").as_slice(),
    )
}

async fn tank_destroyed_mp3() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "audio/mpeg"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        include_bytes!("../web/usa/sounds/tank-destroyed.mp3").as_slice(),
    )
}

async fn mlrs_rocket_mp3() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "audio/mpeg"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        include_bytes!("../web/usa/sounds/mlrs-rocket.mp3").as_slice(),
    )
}

async fn soldier_shoot_mp3() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "audio/mpeg"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        include_bytes!("../web/usa/sounds/soldier-shoot.mp3").as_slice(),
    )
}

async fn building_mp3() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "audio/mpeg"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        include_bytes!("../web/usa/sounds/building.mp3").as_slice(),
    )
}

pub async fn index() -> Html<&'static str> {
    Html(include_str!("index.html"))
}
