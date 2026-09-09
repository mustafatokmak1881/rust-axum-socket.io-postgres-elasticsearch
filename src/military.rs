use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode, header},
};

use axum_extra::extract::CookieJar;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{FromRow, PgConnection};
use uuid::Uuid;

use crate::{
    auth::repository::{self, CurrentUser},
    error::AppError,
    security::hash_token,
    state::SharedState,
};

// Referans hareket değeri: 18 dakika / kare.
// TEST için 540 kat hızlandırılmıştır: yaklaşık 2 saniye / kare.
// Normal referans hareket için bunu 1.0 yap.
const MOVEMENT_SPEED: f64 = 540.0;
const SPEAR_SECONDS_PER_TILE: f64 = 18.0 * 60.0;

#[derive(Deserialize)]
pub struct AttackRequest {
    pub request_id: Uuid,
    pub target_id: Uuid,
    pub spears: i64,
}

#[derive(FromRow)]
struct MapVillage {
    id: Uuid,
    owner_id: Uuid,
    world_id: Uuid,
    x: i32,
    y: i32,
}

#[derive(FromRow)]
struct Attack {
    id: Uuid,
    owner_id: Uuid,
    source_id: Uuid,
    target_id: Uuid,

    arrival_job_id: Uuid,
    return_job_id: Option<Uuid>,

    sent_spears: i64,
    surviving_spears: Option<i64>,

    travel_seconds: i32,
    arrives_at: DateTime<Utc>,
    returns_at: Option<DateTime<Utc>>,
    status: String,
}

async fn current_user(
    state: &SharedState,
    jar: &CookieJar,
) -> Result<CurrentUser, AppError> {
    let cookie = jar
        .get(state.config.session_cookie_name())
        .ok_or(AppError::Unauthorized)?;

    repository::find_session_user(
        &state.db,
        &hash_token(cookie.value()),
    )
    .await?
    .ok_or(AppError::Unauthorized)
}

fn check_origin(
    state: &SharedState,
    headers: &HeaderMap,
) -> Result<(), AppError> {
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());

    if origin != Some(state.config.app_origin.as_str()) {
        return Err(AppError::Forbidden);
    }

    Ok(())
}

/// Oyuncunun köydeki askerleri ve son 20 saldırısı.
/// Başka oyuncuların köydeki askerlerini paylaşmaz.
pub async fn bootstrap(
    State(state): State<SharedState>,
    jar: CookieJar,
) -> Result<impl axum::response::IntoResponse, AppError> {
    let user = current_user(&state, &jar).await?;

    let army = sqlx::query_as::<_, (Uuid, i64)>(
        r#"
        SELECT v.id, a.spears
        FROM villages v
        JOIN village_armies a ON a.village_id = v.id
        WHERE v.owner_id = $1
        "#,
    )
    .bind(user.id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let attacks: Vec<Value> = sqlx::query_scalar(
        r#"
        SELECT jsonb_build_object(
            'id', a.id,
            'target_name', v.name,
            'target_x', v.x,
            'target_y', v.y,
            'sent_spears', a.sent_spears,
            'surviving_spears', a.surviving_spears,
            'defender_before', a.defender_before,
            'defender_after', a.defender_after,
            'status', a.status,
            'arrives_at', a.arrives_at,
            'returns_at', a.returns_at,
            'resolved_at', a.resolved_at,
            'returned_at', a.returned_at
        )
        FROM army_attacks a
        JOIN villages v ON v.id = a.target_id
        WHERE a.owner_id = $1
        ORDER BY a.departed_at DESC, a.id DESC
        LIMIT 20
        "#,
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;

    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "source_id": army.0,
            "spears": army.1,
            "seconds_per_tile":
                SPEAR_SECONDS_PER_TILE / MOVEMENT_SPEED,
            "attacks": attacks
        })),
    ))
}

/// Saldırıyı oluşturur ve birlikleri anında köyden çıkarır.
pub async fn send_attack(
    State(state): State<SharedState>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(input): Json<AttackRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    check_origin(&state, &headers)?;
    let user = current_user(&state, &jar).await?;

    if !(1..=1_000_000).contains(&input.spears) {
        return Err(AppError::BadRequest(
            "Mızrakçı sayısı 1–1.000.000 arasında olmalı.",
        ));
    }

    let mut tx = state.db.begin().await?;

    // Bütün ordu harcamaları kaynak köy kilidiyle korunur.
    let source = sqlx::query_as::<_, MapVillage>(
        r#"
        SELECT id, owner_id, world_id, x, y
        FROM villages
        WHERE owner_id = $1
        FOR UPDATE
        "#,
    )
    .bind(user.id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;

    // Aynı request_id ile tekrar gönderilirse ikinci ordu çıkmaz.
    let previous = sqlx::query_as::<_, Attack>(
        "SELECT * FROM army_attacks WHERE id = $1",
    )
    .bind(input.request_id)
    .fetch_optional(&mut *tx)
    .await?;

    if let Some(previous) = previous {
        if previous.owner_id != user.id
            || previous.source_id != source.id
            || previous.target_id != input.target_id
            || previous.sent_spears != input.spears
        {
            return Err(AppError::BadRequest(
                "İstek kimliği farklı bir komutta kullanılmış.",
            ));
        }

        tx.commit().await?;

        return Ok((
            StatusCode::OK,
            Json(json!({
                "id": previous.id,
                "arrives_at": previous.arrives_at
            })),
        ));
    }

    let target = sqlx::query_as::<_, MapVillage>(
        r#"
        SELECT id, owner_id, world_id, x, y
        FROM villages
        WHERE id = $1
        "#,
    )
    .bind(input.target_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;

    if source.id == target.id || source.owner_id == target.owner_id {
        return Err(AppError::BadRequest(
            "Kendi köyüne saldıramazsın.",
        ));
    }

    if source.world_id != target.world_id {
        return Err(AppError::BadRequest(
            "Hedef köy aynı dünyada olmalı.",
        ));
    }

    let dx = f64::from(target.x - source.x);
    let dy = f64::from(target.y - source.y);
    let distance = dx.hypot(dy);

    let travel_seconds = (
        distance * SPEAR_SECONDS_PER_TILE / MOVEMENT_SPEED
    )
        .ceil()
        .max(1.0) as i32;

    let updated = sqlx::query(
        r#"
        UPDATE village_armies
        SET spears = spears - $2
        WHERE village_id = $1
          AND spears >= $2
        "#,
    )
    .bind(source.id)
    .bind(input.spears)
    .execute(&mut *tx)
    .await?;

    if updated.rows_affected() != 1 {
        return Err(AppError::BadRequest(
            "Köyünde yeterli mızrakçı yok.",
        ));
    }

    let departed_at: DateTime<Utc> =
        sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(&mut *tx)
            .await?;

    let arrives_at =
        departed_at + chrono::Duration::seconds(i64::from(travel_seconds));

    let arrival_job_id = Uuid::new_v4();

    sqlx::query(
        r#"
        INSERT INTO scheduled_jobs (id, kind, payload, run_at)
        VALUES (
            $1,
            'army.attack.arrive.v1',
            '{}'::jsonb,
            $2
        )
        "#,
    )
    .bind(arrival_job_id)
    .bind(arrives_at)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO army_attacks (
            id,
            owner_id,
            source_id,
            target_id,
            arrival_job_id,
            sent_spears,
            travel_seconds,
            departed_at,
            arrives_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(input.request_id)
    .bind(user.id)
    .bind(source.id)
    .bind(target.id)
    .bind(arrival_job_id)
    .bind(input.spears)
    .bind(travel_seconds)
    .bind(departed_at)
    .bind(arrives_at)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "id": input.request_id,
            "arrives_at": arrives_at,
            "travel_seconds": travel_seconds
        })),
    ))
}

/// GELİŞTİRME savaş hesabı:
/// - Mızrakçı saldırı: 10
/// - Mızrakçı piyade savunması: 35
/// - Zayıf taraf tamamen kaybeder.
/// - Güçlü taraf oransal, yukarı yuvarlanan kayıp verir.
/// Bu, Tribal Wars'ın gerçek kayıp formülü değildir.
fn resolve_combat(attacker: i64, defender: i64) -> (i64, i64) {
    let attack_power = i128::from(attacker) * 10;
    let defense_power = i128::from(defender) * 35;

    if defense_power == 0 {
        return (attacker, 0);
    }

    if attack_power == defense_power {
        return (0, 0);
    }

    if attack_power > defense_power {
        let losses = (
            i128::from(attacker) * defense_power
            + attack_power - 1
        ) / attack_power;

        (attacker - losses as i64, 0)
    } else {
        let losses = (
            i128::from(defender) * attack_power
            + defense_power - 1
        ) / defense_power;

        (0, defender - losses as i64)
    }
}

/// Worker'ın açtığı transaction içinde çalışır.
/// scheduled_jobs.completed_at dış handler tarafından güncellenir.
pub async fn execute_job(
    connection: &mut PgConnection,
    job_id: Uuid,
) -> Result<(), AppError> {
    let attack = sqlx::query_as::<_, Attack>(
        r#"
        SELECT *
        FROM army_attacks
        WHERE arrival_job_id = $1 OR return_job_id = $1
        FOR UPDATE
        "#,
    )
    .bind(job_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(AppError::NotFound)?;

    // İki köy her zaman UUID sırasıyla kilitlenir.
    // Karşılıklı saldırılar ters köy kilidi sırası oluşturmaz.
    sqlx::query(
        r#"
        SELECT id
        FROM villages
        WHERE id = $1 OR id = $2
        ORDER BY id
        FOR UPDATE
        "#,
    )
    .bind(attack.source_id)
    .bind(attack.target_id)
    .fetch_all(&mut *connection)
    .await?;

    let now: DateTime<Utc> =
        sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(&mut *connection)
            .await?;

    if job_id == attack.arrival_job_id {
        if attack.status != "outbound" {
            return Ok(());
        }

        if now < attack.arrives_at {
            return Err(AppError::BadRequest("Job is not due yet"));
        }

        let defenders: i64 = sqlx::query_scalar(
            r#"
            SELECT spears
            FROM village_armies
            WHERE village_id = $1
            FOR UPDATE
            "#,
        )
        .bind(attack.target_id)
        .fetch_one(&mut *connection)
        .await?;

        let (survivors, defenders_after) =
            resolve_combat(attack.sent_spears, defenders);

        sqlx::query(
            r#"
            UPDATE village_armies
            SET spears = $2
            WHERE village_id = $1
            "#,
        )
        .bind(attack.target_id)
        .bind(defenders_after)
        .execute(&mut *connection)
        .await?;

        let mut return_job_id = None;
        let mut returns_at = None;

        if survivors > 0 {
            let return_id = Uuid::new_v4();

            // Gidiş süresi kadar dönüş.
            // Worker gecikmesi dönüş yolculuğunu yeniden uzatmaz.
            let return_time = attack.arrives_at
                + chrono::Duration::seconds(
                    i64::from(attack.travel_seconds),
                );

            sqlx::query(
                r#"
                INSERT INTO scheduled_jobs (id, kind, payload, run_at)
                VALUES (
                    $1,
                    'army.attack.return.v1',
                    '{}'::jsonb,
                    $2
                )
                "#,
            )
            .bind(return_id)
            .bind(return_time)
            .execute(&mut *connection)
            .await?;

            return_job_id = Some(return_id);
            returns_at = Some(return_time);
        }

        sqlx::query(
            r#"
            UPDATE army_attacks
            SET surviving_spears = $2,
                defender_before = $3,
                defender_after = $4,
                resolved_at = $5,
                return_job_id = $6,
                returns_at = $7,
                status = $8
            WHERE id = $1
            "#,
        )
        .bind(attack.id)
        .bind(survivors)
        .bind(defenders)
        .bind(defenders_after)
        .bind(now)
        .bind(return_job_id)
        .bind(returns_at)
        .bind(if survivors > 0 { "returning" } else { "completed" })
        .execute(&mut *connection)
        .await?;

        return Ok(());
    }

    if attack.return_job_id == Some(job_id) {
        if attack.status != "returning" {
            return Ok(());
        }

        let returns_at = attack.returns_at.ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "Missing return time for attack {}",
                attack.id
            ))
        })?;

        if now < returns_at {
            return Err(AppError::BadRequest("Job is not due yet"));
        }

        let survivors = attack.surviving_spears.ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "Missing survivor count for attack {}",
                attack.id
            ))
        })?;

        sqlx::query(
            r#"
            UPDATE village_armies
            SET spears = spears + $2
            WHERE village_id = $1
            "#,
        )
        .bind(attack.source_id)
        .bind(survivors)
        .execute(&mut *connection)
        .await?;

        sqlx::query(
            r#"
            UPDATE army_attacks
            SET status = 'completed',
                returned_at = $2
            WHERE id = $1
            "#,
        )
        .bind(attack.id)
        .bind(now)
        .execute(&mut *connection)
        .await?;
    }

    Ok(())
}