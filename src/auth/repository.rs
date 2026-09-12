use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(FromRow)]
pub struct LoginFlow {
    pub nonce: String,
    pub pkce_verifier: String,
}

#[derive(Serialize, FromRow)]
pub struct CurrentUser {
    pub id: Uuid,
    pub email: String,
    pub email_verified: bool,
    pub created_at: DateTime<Utc>,
    pub faction: Option<String>,
}

pub async fn create_login_flow(
    db: &PgPool,
    state_hash: &str,
    browser_token_hash: &str,
    nonce: &str,
    pkce_verifier: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO oauth_login_flows (
            state_hash,
            browser_token_hash,
            nonce,
            pkce_verifier,
            expires_at
        )
        VALUES ($1, $2, $3, $4, NOW() + INTERVAL '10 minutes')
        "#,
    )
    .bind(state_hash)
    .bind(browser_token_hash)
    .bind(nonce)
    .bind(pkce_verifier)
    .execute(db)
    .await?;

    Ok(())
}

pub async fn consume_login_flow(
    db: &PgPool,
    state_hash: &str,
    browser_token_hash: &str,
) -> Result<Option<LoginFlow>, sqlx::Error> {
    sqlx::query_as::<_, LoginFlow>(
        r#"
        DELETE FROM oauth_login_flows
        WHERE state_hash = $1
          AND browser_token_hash = $2
          AND expires_at > NOW()
        RETURNING nonce, pkce_verifier
        "#,
    )
    .bind(state_hash)
    .bind(browser_token_hash)
    .fetch_optional(db)
    .await
}

pub async fn create_session(
    db: &PgPool,
    google_sub: &str,
    email: &str,
    token_hash: &str,
    previous_token_hash: Option<&str>,
) -> Result<(), sqlx::Error> {
    let mut transaction = db.begin().await?;

    let user_id: Uuid = sqlx::query_scalar(
        r#"
        INSERT INTO users (
            id,
            google_sub,
            email,
            email_verified
        )
        VALUES ($1, $2, $3, TRUE)
        ON CONFLICT (google_sub)
        DO UPDATE SET
            email = EXCLUDED.email,
            email_verified = EXCLUDED.email_verified,
            updated_at = NOW()
        RETURNING id
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(google_sub)
    .bind(email)
    .fetch_one(&mut *transaction)
    .await?;

    // Aynı tarayıcıdaki önceki oturumu iptal et.
    if let Some(previous_token_hash) = previous_token_hash {
        sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
            .bind(previous_token_hash)
            .execute(&mut *transaction)
            .await?;
    }

    sqlx::query(
        r#"
        INSERT INTO sessions (
            token_hash,
            user_id,
            expires_at
        )
        VALUES ($1, $2, NOW() + INTERVAL '7 days')
        "#,
    )
    .bind(token_hash)
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;

    transaction.commit().await
}

pub async fn find_session_user(
    db: &PgPool,
    token_hash: &str,
) -> Result<Option<CurrentUser>, sqlx::Error> {
    sqlx::query_as::<_, CurrentUser>(
        r#"
        SELECT
            u.id,
            u.email,
            u.email_verified,
            u.created_at,
            u.faction
        FROM sessions s
        JOIN users u ON u.id = s.user_id
        WHERE s.token_hash = $1
          AND s.expires_at > NOW()
        "#,
    )
    .bind(token_hash)
    .fetch_optional(db)
    .await
}

pub async fn delete_session(db: &PgPool, token_hash: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
        .bind(token_hash)
        .execute(db)
        .await?;

    Ok(())
}

pub async fn cleanup_expired(db: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM oauth_login_flows WHERE expires_at <= NOW()")
        .execute(db)
        .await?;

    sqlx::query("DELETE FROM sessions WHERE expires_at <= NOW()")
        .execute(db)
        .await?;

    Ok(())
}
