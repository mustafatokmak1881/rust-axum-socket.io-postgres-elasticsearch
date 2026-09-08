use axum_extra::extract::cookie::{Cookie, SameSite};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};
use time::Duration;

pub fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);

    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn hash_token(value: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(value.as_bytes()))
}

pub fn auth_cookie(
    name: &'static str,
    value: String,
    secure: bool,
    lifetime: Duration,
) -> Cookie<'static> {
    Cookie::build((name, value))
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .max_age(lifetime)
        .build()
}

pub fn removal_cookie(name: &'static str, secure: bool) -> Cookie<'static> {
    Cookie::build((name, ""))
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .build()
}
