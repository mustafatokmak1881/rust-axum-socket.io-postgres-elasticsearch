use super::repository;

use crate::{
    error::AppError,
    security::{auth_cookie, hash_token, random_token, removal_cookie},
    state::SharedState,
};

use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::Redirect,
};

use axum_extra::extract::CookieJar;

use openidconnect::{
    AccessTokenHash, AuthorizationCode, CsrfToken, Nonce, OAuth2TokenResponse, PkceCodeChallenge,
    PkceCodeVerifier, Scope, core::CoreAuthenticationFlow, reqwest::async_http_client,
};

use serde::Deserialize;
use time::Duration;

#[derive(Deserialize)]
pub struct GoogleCallback {
    pub code: String,
    pub state: String,
}

pub async fn google_login(
    State(state): State<SharedState>,
    jar: CookieJar,
) -> Result<(CookieJar, Redirect), AppError> {
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

    let (authorization_url, csrf_token, nonce) = state
        .google
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .add_scope(Scope::new("email".to_owned()))
        .set_pkce_challenge(pkce_challenge)
        .url();

    let browser_token = random_token();

    repository::create_login_flow(
        &state.db,
        &hash_token(csrf_token.secret()),
        &hash_token(&browser_token),
        nonce.secret(),
        pkce_verifier.secret(),
    )
    .await?;

    let jar = jar.add(auth_cookie(
        state.config.oauth_cookie_name(),
        browser_token,
        state.config.cookie_secure,
        Duration::minutes(10),
    ));

    Ok((jar, Redirect::temporary(authorization_url.as_str())))
}

pub async fn google_callback(
    State(state): State<SharedState>,
    jar: CookieJar,
    Query(query): Query<GoogleCallback>,
) -> Result<(CookieJar, Redirect), AppError> {
    let browser_token = jar
        .get(state.config.oauth_cookie_name())
        .ok_or(AppError::Unauthorized)?
        .value()
        .to_owned();

    // Tek kullanımlık state + tarayıcı bağı kontrolü.
    let flow = repository::consume_login_flow(
        &state.db,
        &hash_token(&query.state),
        &hash_token(&browser_token),
    )
    .await?
    .ok_or(AppError::Unauthorized)?;

    let token_response = state
        .google
        .exchange_code(AuthorizationCode::new(query.code))
        .set_pkce_verifier(PkceCodeVerifier::new(flow.pkce_verifier))
        .request_async(async_http_client)
        .await
        .map_err(|_| AppError::Unauthorized)?;

    let id_token = token_response
        .extra_fields()
        .id_token()
        .ok_or(AppError::Unauthorized)?;

    let verifier = state.google.id_token_verifier();
    let expected_nonce = Nonce::new(flow.nonce);

    // İmza, issuer, audience, expiration ve nonce doğrulanır.
    let claims = id_token
        .claims(&verifier, &expected_nonce)
        .map_err(|_| AppError::Unauthorized)?;

    if let Some(expected_access_token_hash) = claims.access_token_hash() {
        let signing_algorithm = id_token.signing_alg().map_err(|_| AppError::Unauthorized)?;

        let actual_access_token_hash =
            AccessTokenHash::from_token(token_response.access_token(), &signing_algorithm)
                .map_err(|_| AppError::Unauthorized)?;

        if actual_access_token_hash != *expected_access_token_hash {
            return Err(AppError::Unauthorized);
        }
    }

    if claims.email_verified() != Some(true) {
        return Err(AppError::Unauthorized);
    }

    let email = claims
        .email()
        .ok_or(AppError::Unauthorized)?
        .as_str()
        .to_owned();

    let google_sub = claims.subject().as_str().to_owned();

    let session_token = random_token();

    let previous_token_hash = jar
        .get(state.config.session_cookie_name())
        .map(|cookie| hash_token(cookie.value()));

    repository::create_session(
        &state.db,
        &google_sub,
        &email,
        &hash_token(&session_token),
        previous_token_hash.as_deref(),
    )
    .await?;

    let jar = jar
        .remove(removal_cookie(
            state.config.oauth_cookie_name(),
            state.config.cookie_secure,
        ))
        .add(auth_cookie(
            state.config.session_cookie_name(),
            session_token,
            state.config.cookie_secure,
            Duration::days(7),
        ));

    Ok((jar, Redirect::to("/game")))
}

pub async fn me(
    State(state): State<SharedState>,
    jar: CookieJar,
) -> Result<Json<repository::CurrentUser>, AppError> {
    let session_token = jar
        .get(state.config.session_cookie_name())
        .ok_or(AppError::Unauthorized)?
        .value();

    let user = repository::find_session_user(&state.db, &hash_token(session_token))
        .await?
        .ok_or(AppError::Unauthorized)?;

    Ok(Json(user))
}

pub async fn logout(
    State(state): State<SharedState>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<(CookieJar, StatusCode), AppError> {
    // Cookie tabanlı, durum değiştiren endpoint için Origin kontrolü.
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());

    if origin != Some(state.config.app_origin.as_str()) {
        return Err(AppError::Forbidden);
    }

    if let Some(cookie) = jar.get(state.config.session_cookie_name()) {
        repository::delete_session(&state.db, &hash_token(cookie.value())).await?;
    }

    let jar = jar.remove(removal_cookie(
        state.config.session_cookie_name(),
        state.config.cookie_secure,
    ));

    Ok((jar, StatusCode::NO_CONTENT))
}
