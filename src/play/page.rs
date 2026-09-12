use axum::{
    extract::State,
    http::header,
    response::{Html, IntoResponse, Redirect, Response},
};
use axum_extra::extract::CookieJar;

use crate::{
    error::AppError,
    security::hash_token,
    state::SharedState,
    store::users,
};

pub async fn play(
    State(state): State<SharedState>,
    jar: CookieJar,
) -> Result<Response, AppError> {
    let Some(cookie) = jar.get(state.config.session_cookie_name()) else {
        return Ok(Redirect::to("/auth/google").into_response());
    };

    let user = users::find_session_user(&state.redis, &hash_token(cookie.value())).await?;
    if user.is_none() {
        return Ok(Redirect::to("/auth/google").into_response());
    }

    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Html(include_str!("play.html")),
    )
        .into_response())
}
