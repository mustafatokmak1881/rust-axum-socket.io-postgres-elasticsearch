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
        "headquarters" => include_str!("building-headquarters.svg"),
        "barracks" => include_str!("building-barracks.svg"),
        "stable" => include_str!("building-stable.svg"),
        "workshop" => include_str!("building-workshop.svg"),
        "academy" => include_str!("building-academy.svg"),
        "smithy" => include_str!("building-smithy.svg"),
        "rally_point" => include_str!("building-rally_point.svg"),
        "statue" => include_str!("building-statue.svg"),
        "market" => include_str!("building-market.svg"),
        "timber" => include_str!("building-timber.svg"),
        "clay" => include_str!("building-clay.svg"),
        "iron" => include_str!("building-iron.svg"),
        "farm" => include_str!("building-farm.svg"),
        "warehouse" => include_str!("building-warehouse.svg"),
        "hiding_place" => include_str!("building-hiding_place.svg"),
        "wall" => include_str!("building-wall.svg"),
        "generic" => include_str!("building-generic.svg"),
        _ => return None,
    })
}

fn svg_response(svg: &'static str) -> impl axum::response::IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "image/svg+xml; charset=utf-8",
        )],
        svg,
    )
}

pub async fn building_kind_art(
    axum::extract::Path(kind): axum::extract::Path<String>,
) -> Result<impl axum::response::IntoResponse, crate::error::AppError> {
    let svg = building_svg(&kind).ok_or(crate::error::AppError::NotFound)?;
    Ok(svg_response(svg))
}

pub async fn headquarters_art() -> impl axum::response::IntoResponse {
    svg_response(include_str!("building-headquarters.svg"))
}

pub async fn timber_art() -> impl axum::response::IntoResponse {
    svg_response(include_str!("building-timber.svg"))
}

pub async fn warehouse_art() -> impl axum::response::IntoResponse {
    svg_response(include_str!("building-warehouse.svg"))
}

pub async fn generic_building_art() -> impl axum::response::IntoResponse {
    svg_response(include_str!("building-generic.svg"))
}
