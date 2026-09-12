pub mod api;

use axum::{
    extract::State,
    http::header,
    response::{Html, IntoResponse, Redirect, Response},
};
use axum_extra::extract::CookieJar;

use crate::{auth::repository, error::AppError, security::hash_token, state::SharedState};

pub async fn index() -> Html<&'static str> {
    Html(include_str!("index.html"))
}

pub async fn game(State(state): State<SharedState>, jar: CookieJar) -> Result<Response, AppError> {
    if let Some(cookie) = jar.get(state.config.session_cookie_name()) {
        let user = repository::find_session_user(&state.db, &hash_token(cookie.value())).await?;

        if user.is_some() {
            let mut response = Html(include_str!("game.html")).into_response();

            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());

            return Ok(response);
        }
    }

    Ok(Redirect::to("/").into_response())
}

pub async fn stylesheet() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("game.css"),
    )
}

pub async fn javascript() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("game.js"),
    )
}

pub async fn village_terrain_art() -> Response {
    (
        [(header::CONTENT_TYPE, "image/jpeg")],
        include_bytes!("village-terrain.jpg").as_slice(),
    )
        .into_response()
}

pub async fn building_art() -> impl axum::response::IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "image/svg+xml; charset=utf-8",
        )],
        include_str!("buildings.svg"),
    )
}

fn building_svg(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "stable" => include_str!("building-stable.svg"),
        "workshop" => include_str!("building-workshop.svg"),
        "academy" => include_str!("building-academy.svg"),
        "smithy" => include_str!("building-smithy.svg"),
        "statue" => include_str!("building-statue.svg"),
        "market" => include_str!("building-market.svg"),
        "wall" => include_str!("building-wall.svg"),
        "generic" => include_str!("building-generic.svg"),
        _ => return None,
    })
}

fn building_png(kind: &str) -> Option<&'static [u8]> {
    Some(match kind {
        "headquarters" => include_bytes!("command-center.png").as_slice(),
        "barracks" => include_bytes!("barracks.png").as_slice(),
        "clay" => include_bytes!("kil-ocagi.png").as_slice(),
        "timber" => include_bytes!("oduncu.png").as_slice(),
        "iron" => include_bytes!("demir-madeni.png").as_slice(),
        "rally_point" => include_bytes!("ictima-meydani.png").as_slice(),
        "farm" => include_bytes!("ciftlik.png").as_slice(),
        "hiding_place" => include_bytes!("gizli-depo.png").as_slice(),
        "warehouse" => include_bytes!("ambar.png").as_slice(),
        _ => return None,
    })
}

fn svg_response(svg: &'static str) -> Response {
    (
        [(header::CONTENT_TYPE, "image/svg+xml; charset=utf-8")],
        svg,
    )
        .into_response()
}

fn png_response(png: &'static [u8]) -> Response {
    ([(header::CONTENT_TYPE, "image/png")], png).into_response()
}

pub async fn building_kind_art(
    axum::extract::Path(kind): axum::extract::Path<String>,
) -> Result<Response, crate::error::AppError> {
    if let Some(png) = building_png(&kind) {
        return Ok(png_response(png));
    }

    let svg = building_svg(&kind).ok_or(crate::error::AppError::NotFound)?;
    Ok(svg_response(svg))
}
