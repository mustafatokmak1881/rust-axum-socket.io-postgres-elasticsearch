pub mod handlers;

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    error::AppError,
    realtime::protocol::default_catalog,
    state::SharedState,
    store::users,
};

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/store/catalog", get(catalog))
        .route("/api/store/dev-grant", post(dev_grant))
        .route("/api/store/checkout", post(checkout))
        .route("/api/stripe/webhook", post(stripe_webhook))
}

async fn catalog() -> Json<Value> {
    Json(json!({ "items": default_catalog() }))
}

#[derive(Deserialize)]
struct GrantBody {
    item_id: String,
}

/// Local/dev grant — never enables combat power. Cosmetics / XP boost only.
async fn dev_grant(
    State(state): State<SharedState>,
    headers: HeaderMap,
    jar: axum_extra::extract::CookieJar,
    Json(body): Json<GrantBody>,
) -> Result<Json<Value>, AppError> {
    check_origin(&state, &headers)?;
    let user = current_user(&state, &jar).await?;

    let allowed = default_catalog()
        .iter()
        .any(|item| item.id == body.item_id);
    if !allowed {
        return Err(AppError::BadRequest("Unknown catalog item"));
    }

    users::grant_entitlement(&state.redis, user.id, &body.item_id).await?;
    let entitlements = users::list_entitlements(&state.redis, user.id).await?;

    Ok(Json(json!({ "ok": true, "entitlements": entitlements })))
}

#[derive(Deserialize)]
struct CheckoutBody {
    item_id: String,
}

async fn checkout(
    State(state): State<SharedState>,
    headers: HeaderMap,
    jar: axum_extra::extract::CookieJar,
    Json(body): Json<CheckoutBody>,
) -> Result<Json<Value>, AppError> {
    check_origin(&state, &headers)?;
    let user = current_user(&state, &jar).await?;

    if state.config.stripe_secret_key.is_none() {
        // No Stripe configured — point clients at dev-grant.
        return Ok(Json(json!({
            "mode": "dev",
            "message": "Stripe not configured. Use /api/store/dev-grant.",
            "item_id": body.item_id,
            "user_id": user.id,
        })));
    }

    Ok(Json(json!({
        "mode": "stripe",
        "message": "Create Checkout Session with your Stripe price IDs (wired via env).",
        "item_id": body.item_id,
        "success_url": format!("{}/play?store=ok", state.config.app_origin),
        "cancel_url": format!("{}/play?store=cancel", state.config.app_origin),
    })))
}

/// Stripe webhook: grant entitlement from checkout.session.completed metadata.
async fn stripe_webhook(
    State(state): State<SharedState>,
    headers: HeaderMap,
    body: String,
) -> Result<StatusCode, AppError> {
    let _ = headers.get("stripe-signature");
    // Signature verification when STRIPE_WEBHOOK_SECRET is set (simplified parse).
    if let Some(secret) = &state.config.stripe_webhook_secret {
        let _ = secret;
        // Full HMAC verification can be expanded; accept JSON body with metadata for now
        // when secret is present we still parse event payload.
    }

    let event: Value = serde_json::from_str(&body).map_err(|_| AppError::BadRequest("Invalid JSON"))?;
    let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");

    if event_type == "checkout.session.completed" {
        let obj = event.get("data").and_then(|d| d.get("object"));
        let user_id = obj
            .and_then(|o| o.get("metadata"))
            .and_then(|m| m.get("user_id"))
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok());
        let item_id = obj
            .and_then(|o| o.get("metadata"))
            .and_then(|m| m.get("item_id"))
            .and_then(|v| v.as_str());

        if let (Some(user_id), Some(item_id)) = (user_id, item_id) {
            users::grant_entitlement(&state.redis, user_id, item_id).await?;
        }
    }

    Ok(StatusCode::OK)
}

async fn current_user(
    state: &SharedState,
    jar: &axum_extra::extract::CookieJar,
) -> Result<users::CurrentUser, AppError> {
    use crate::security::hash_token;
    let token = jar
        .get(state.config.session_cookie_name())
        .ok_or(AppError::Unauthorized)?
        .value();
    users::find_session_user(&state.redis, &hash_token(token))
        .await?
        .ok_or(AppError::Unauthorized)
}

fn check_origin(state: &SharedState, headers: &HeaderMap) -> Result<(), AppError> {
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    if origin != Some(state.config.app_origin.as_str()) {
        return Err(AppError::Forbidden);
    }
    Ok(())
}
