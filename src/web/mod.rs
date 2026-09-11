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

pub async fn headquarters_art() -> impl axum::response::IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "image/svg+xml; charset=utf-8",
        )],
        include_str!("building-headquarters.svg"),
    )
}

pub async fn timber_art() -> impl axum::response::IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "image/svg+xml; charset=utf-8",
        )],
        include_str!("building-timber.svg"),
    )
}

pub async fn warehouse_art() -> impl axum::response::IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "image/svg+xml; charset=utf-8",
        )],
        include_str!("building-warehouse.svg"),
    )
}

pub async fn generic_building_art() -> impl axum::response::IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "image/svg+xml; charset=utf-8",
        )],
        include_str!("building-generic.svg"),
    )
}
