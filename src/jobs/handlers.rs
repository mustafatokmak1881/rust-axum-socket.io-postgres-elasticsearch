use crate::{
    error::AppError,
    state::SharedState,
};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::FromRow;
use subtle::ConstantTimeEq;
use uuid::Uuid;

#[derive(FromRow)]
struct ScheduledJob {
    kind: String,
    payload: Value,
    completed_at: Option<DateTime<Utc>>,
    is_due: bool,
}

pub async fn execute(
    State(state): State<SharedState>,
    Path(job_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    let supplied_secret = headers
        .get("x-worker-secret")
        .and_then(|value| value.to_str().ok())
        .ok_or(AppError::Unauthorized)?;

    let authorized: bool = supplied_secret
        .as_bytes()
        .ct_eq(state.config.internal_worker_secret.as_bytes())
        .into();

    if !authorized {
        return Err(AppError::Unauthorized);
    }

    let mut transaction = state.db.begin().await?;

    let job = sqlx::query_as::<_, ScheduledJob>(
        r#"
        SELECT
            kind,
            payload,
            completed_at,
            run_at <= NOW() AS is_due
        FROM scheduled_jobs
        WHERE id = $1
        FOR UPDATE
        "#,
    )
    .bind(job_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;

    // BullMQ aynı işi tekrar teslim edebilir.
    // Tamamlanan işin etkileri ikinci kez uygulanmaz.
    if job.completed_at.is_some() {
        transaction.commit().await?;
        return Ok(StatusCode::NO_CONTENT);
    }

    if !job.is_due {
        return Err(AppError::BadRequest("Job is not due yet"));
    }

    match job.kind.as_str() {
        "system.ping" => {
            tracing::info!(
                %job_id,
                payload = %job.payload,
                "Scheduled test job executed"
            );
        }

        // Sonraki aşama:
        //
        // "battle.resolve.v1" => {
        //     battle_service::resolve(
        //         &mut transaction,
        //         &job.payload,
        //     ).await?;
        // }
        //
        // Savaş sonucu ve completed_at aynı transaction'da yazılmalı.

        _ => {
            return Err(AppError::BadRequest("Unsupported job kind"));
        }
    }

    sqlx::query(
        r#"
        UPDATE scheduled_jobs
        SET completed_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(job_id)
    .execute(&mut *transaction)
    .await?;

    transaction.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}