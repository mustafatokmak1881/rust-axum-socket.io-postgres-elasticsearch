use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
};
use axum_extra::extract::CookieJar;

use crate::{
    auth::repository,
    error::AppError,
    security::hash_token,
    state::SharedState,
};

pub async fn index() -> Redirect {
    Redirect::to("/game")
}

pub async fn game(
    State(state): State<SharedState>,
    jar: CookieJar,
) -> Result<Response, AppError> {
    let Some(cookie) = jar.get(state.config.session_cookie_name()) else {
        return Ok(Redirect::to("/auth/google").into_response());
    };

    let user = repository::find_session_user(
        &state.db,
        &hash_token(cookie.value()),
    )
    .await?;

    if user.is_none() {
        return Ok(Redirect::to("/auth/google").into_response());
    }

    let mut response = Html(include_str!("game.html")).into_response();

    response.headers_mut().insert(
        header::CACHE_CONTROL,
        "no-store".parse().unwrap(),
    );

    Ok(response)
}

pub async fn stylesheet() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("game.css"),
    )
}