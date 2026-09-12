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

/// USA bina görselleri (`src/web/usa/`).
fn usa_building_png(kind: &str) -> Option<&'static [u8]> {
    Some(match kind {
        "headquarters" => include_bytes!("usa/command-center.png").as_slice(),
        "barracks" => include_bytes!("usa/barracks.png").as_slice(),
        "academy" => include_bytes!("usa/strategy-center.png").as_slice(),
        "workshop" => include_bytes!("usa/war-factory.png").as_slice(),
        "timber" => include_bytes!("usa/supply-pile.png").as_slice(),
        "clay" => include_bytes!("usa/fuel-depot.png").as_slice(),
        "iron" => include_bytes!("usa/munitions-plant.png").as_slice(),
        "rally_point" => include_bytes!("usa/staging-area.png").as_slice(),
        "farm" => include_bytes!("usa/cold-fusion-reactor.png").as_slice(),
        "warehouse" => include_bytes!("usa/supply-center.png").as_slice(),
        "hiding_place" => include_bytes!("usa/detention-camp.png").as_slice(),
        _ => return None,
    })
}

/// China görselleri (`src/web/china/`) — yoksa None → USA fallback.
fn china_building_png(_kind: &str) -> Option<&'static [u8]> {
    None
}

/// GLA görselleri (`src/web/gla/`) — yoksa None → USA fallback.
fn gla_building_png(_kind: &str) -> Option<&'static [u8]> {
    None
}

fn building_png(faction: &str, kind: &str) -> Option<&'static [u8]> {
    let faction_art = match faction {
        "china" => china_building_png(kind),
        "gla" => gla_building_png(kind),
        _ => usa_building_png(kind),
    };

    faction_art.or_else(|| usa_building_png(kind))
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
    axum::extract::Path((faction, kind)): axum::extract::Path<(String, String)>,
) -> Result<Response, crate::error::AppError> {
    let faction = faction.to_ascii_lowercase();

    if !matches!(faction.as_str(), "usa" | "china" | "gla") {
        return Err(crate::error::AppError::NotFound);
    }

    if let Some(png) = building_png(&faction, &kind) {
        return Ok(png_response(png));
    }

    let svg = building_svg(&kind).ok_or(crate::error::AppError::NotFound)?;
    Ok(svg_response(svg))
}
