use chrono::{DateTime, Utc};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Redis, keys};
use crate::error::AppError;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserRecord {
    pub id: Uuid,
    pub google_sub: String,
    pub email: String,
    pub email_verified: bool,
    pub created_at: DateTime<Utc>,
    pub faction: Option<String>,
    pub display_name: String,
    pub equipped_flag: Option<String>,
    pub xp: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CurrentUser {
    pub id: Uuid,
    pub email: String,
    pub email_verified: bool,
    pub created_at: DateTime<Utc>,
    pub faction: Option<String>,
    pub display_name: String,
    pub equipped_flag: Option<String>,
    pub xp: i64,
}

impl From<UserRecord> for CurrentUser {
    fn from(value: UserRecord) -> Self {
        Self {
            id: value.id,
            email: value.email,
            email_verified: value.email_verified,
            created_at: value.created_at,
            faction: value.faction,
            display_name: value.display_name,
            equipped_flag: value.equipped_flag,
            xp: value.xp,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct LoginFlow {
    nonce: String,
    pkce_verifier: String,
    browser_token_hash: String,
}

pub async fn create_login_flow(
    redis: &Redis,
    state_hash: &str,
    browser_token_hash: &str,
    nonce: &str,
    pkce_verifier: &str,
) -> Result<(), AppError> {
    let mut conn = redis.clone();
    let payload = LoginFlow {
        nonce: nonce.to_owned(),
        pkce_verifier: pkce_verifier.to_owned(),
        browser_token_hash: browser_token_hash.to_owned(),
    };
    let key = keys::oauth_flow(state_hash);
    let _: () = conn
        .set_ex(key, serde_json::to_string(&payload)?, 600)
        .await?;
    Ok(())
}

pub async fn consume_login_flow(
    redis: &Redis,
    state_hash: &str,
    browser_token_hash: &str,
) -> Result<Option<(String, String)>, AppError> {
    let mut conn = redis.clone();
    let key = keys::oauth_flow(state_hash);
    let raw: Option<String> = conn.get(&key).await?;
    let _: () = conn.del(&key).await?;

    let Some(raw) = raw else {
        return Ok(None);
    };

    let flow: LoginFlow = serde_json::from_str(&raw)?;
    if flow.browser_token_hash != browser_token_hash {
        return Ok(None);
    }

    Ok(Some((flow.nonce, flow.pkce_verifier)))
}

pub async fn upsert_user_and_session(
    redis: &Redis,
    google_sub: &str,
    email: &str,
    token_hash: &str,
    previous_token_hash: Option<&str>,
) -> Result<Uuid, AppError> {
    let mut conn = redis.clone();

    if let Some(previous) = previous_token_hash {
        let _: () = conn.del(keys::session(previous)).await?;
    }

    let google_key = keys::user_by_google(google_sub);
    let existing_id: Option<String> = conn.get(&google_key).await?;

    let user = if let Some(id) = existing_id {
        let user_id = Uuid::parse_str(&id).map_err(|e| AppError::Internal(e.into()))?;
        let mut user = load_user(redis, user_id)
            .await?
            .ok_or(AppError::Internal(anyhow::anyhow!("user missing")))?;
        user.email = email.to_owned();
        user.email_verified = true;
        save_user(redis, &user).await?;
        user
    } else {
        let user = UserRecord {
            id: Uuid::new_v4(),
            google_sub: google_sub.to_owned(),
            email: email.to_owned(),
            email_verified: true,
            created_at: Utc::now(),
            faction: None,
            display_name: email
                .split('@')
                .next()
                .unwrap_or("Commander")
                .chars()
                .take(24)
                .collect(),
            equipped_flag: None,
            xp: 0,
        };
        let _: () = conn.set(&google_key, user.id.to_string()).await?;
        save_user(redis, &user).await?;
        user
    };

    let _: () = conn
        .set_ex(keys::session(token_hash), user.id.to_string(), 7 * 24 * 3600)
        .await?;

    Ok(user.id)
}

pub async fn save_user(redis: &Redis, user: &UserRecord) -> Result<(), AppError> {
    let mut conn = redis.clone();
    let _: () = conn
        .set(keys::user(&user.id.to_string()), serde_json::to_string(user)?)
        .await?;
    Ok(())
}

pub async fn load_user(redis: &Redis, user_id: Uuid) -> Result<Option<UserRecord>, AppError> {
    let mut conn = redis.clone();
    let raw: Option<String> = conn.get(keys::user(&user_id.to_string())).await?;
    Ok(match raw {
        Some(raw) => Some(serde_json::from_str(&raw)?),
        None => None,
    })
}

pub async fn find_session_user(
    redis: &Redis,
    token_hash: &str,
) -> Result<Option<CurrentUser>, AppError> {
    let mut conn = redis.clone();
    let user_id: Option<String> = conn.get(keys::session(token_hash)).await?;
    let Some(user_id) = user_id else {
        return Ok(None);
    };
    let user_id = Uuid::parse_str(&user_id).map_err(|e| AppError::Internal(e.into()))?;
    Ok(load_user(redis, user_id).await?.map(Into::into))
}

pub async fn delete_session(redis: &Redis, token_hash: &str) -> Result<(), AppError> {
    let mut conn = redis.clone();
    let _: () = conn.del(keys::session(token_hash)).await?;
    Ok(())
}

pub async fn list_entitlements(redis: &Redis, user_id: Uuid) -> Result<Vec<String>, AppError> {
    let mut conn = redis.clone();
    let items: Vec<String> = conn.smembers(keys::entitlements(&user_id.to_string())).await?;
    Ok(items)
}

pub async fn grant_entitlement(
    redis: &Redis,
    user_id: Uuid,
    item_id: &str,
) -> Result<(), AppError> {
    let mut conn = redis.clone();
    let _: () = conn
        .sadd(keys::entitlements(&user_id.to_string()), item_id)
        .await?;
    Ok(())
}

pub async fn has_entitlement(
    redis: &Redis,
    user_id: Uuid,
    item_id: &str,
) -> Result<bool, AppError> {
    let mut conn = redis.clone();
    Ok(conn
        .sismember(keys::entitlements(&user_id.to_string()), item_id)
        .await?)
}
