pub mod page;

use std::path::{Component, Path, PathBuf};

use axum::{
    Router,
    body::Body,
    extract::Path as AxumPath,
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
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
        .route("/assets/models/war-factory.obj", get(war_factory_obj))
        .route("/assets/models/war-factory.mtl", get(war_factory_mtl))
        .route("/assets/models/barracks.stl", get(barracks_stl))
        .route("/assets/models/ranger/{*path}", get(ranger_asset))
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

fn ranger_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/web/usa/ranger")
}

fn content_type_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "gltf" => "model/gltf+json",
        "glb" => "model/gltf-binary",
        "bin" => "application/octet-stream",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Serve `src/web/usa/ranger/**` (scene.gltf + scene.bin + textures).
async fn ranger_asset(AxumPath(path): AxumPath<String>) -> Response {
    let root = ranger_root();
    let mut safe = PathBuf::new();
    for component in Path::new(&path).components() {
        match component {
            Component::Normal(part) => safe.push(part),
            _ => {
                return (StatusCode::BAD_REQUEST, "invalid path").into_response();
            }
        }
    }
    if safe.as_os_str().is_empty() {
        return (StatusCode::NOT_FOUND, "missing").into_response();
    }

    let full = root.join(&safe);
    let Ok(bytes) = tokio::fs::read(&full).await else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type_for(&full))
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
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
