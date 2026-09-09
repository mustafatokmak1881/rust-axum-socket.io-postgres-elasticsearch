use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
};
use axum_extra::extract::CookieJar;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{FromRow, PgConnection};
use uuid::Uuid;

use crate::{
    auth::repository::{self, CurrentUser},
    error::AppError,
    security::hash_token,
    state::SharedState,
};

#[derive(Serialize, FromRow)]
struct Village {
    id: Uuid,
    name: String,
    wood: i64,
}

#[derive(Serialize, FromRow)]
struct Building {
    kind: String,
    level: i32,
}

#[derive(Serialize, FromRow)]
struct Upgrade {
    job_id: Uuid,
    building_kind: String,
    target_level: i32,
    run_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
pub struct RenameRequest {
    name: String,
}

async fn current_user(state: &SharedState, jar: &CookieJar) -> Result<CurrentUser, AppError> {
    let token = jar
        .get(state.config.session_cookie_name())
        .ok_or(AppError::Unauthorized)?
        .value();

    repository::find_session_user(&state.db, &hash_token(token))
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

pub async fn bootstrap(
    State(state): State<SharedState>,
    jar: CookieJar,
) -> Result<Json<Value>, AppError> {
    let user = current_user(&state, &jar).await?;

    let mut tx = state.db.begin().await?;

    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;

    let village =
        sqlx::query_as::<_, Village>("SELECT id, name, wood FROM villages WHERE owner_id = $1")
            .bind(user.id)
            .fetch_optional(&mut *tx)
            .await?;

    let mut buildings: Vec<Building> = Vec::new();
    let mut upgrades: Vec<Upgrade> = Vec::new();

    if let Some(village) = &village {
        buildings = sqlx::query_as::<_, Building>(
            r#"
            SELECT kind, level
            FROM village_buildings
            WHERE village_id = $1
            ORDER BY kind
            "#,
        )
        .bind(village.id)
        .fetch_all(&mut *tx)
        .await?;

        upgrades = sqlx::query_as::<_, Upgrade>(
            r#"
            SELECT
                u.job_id,
                u.building_kind,
                u.target_level,
                j.run_at,
                u.completed_at
            FROM building_upgrades u
            JOIN scheduled_jobs j ON j.id = u.job_id
            WHERE u.village_id = $1
            ORDER BY
                (u.completed_at IS NULL) DESC,
                j.created_at DESC,
                u.job_id DESC
            LIMIT 30
            "#,
        )
        .bind(village.id)
        .fetch_all(&mut *tx)
        .await?;
    }

    let server_time: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *tx)
        .await?;

    tx.commit().await?;

    Ok(Json(json!({
        "user": user,
        "village": village,
        "buildings": buildings,
        "upgrades": upgrades,
        "server_time": server_time,
        "rules": {
            "max_level": 20,
            "wood_per_target_level": 100,
            "seconds_per_target_level": 15
        }
    })))
}

pub async fn create_village(
    State(state): State<SharedState>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<StatusCode, AppError> {
    check_origin(&state, &headers)?;
    let user = current_user(&state, &jar).await?;

    let world_id = Uuid::from_u128(1);
    let mut tx = state.db.begin().await?;

    // Başlangıç köyü oluşturma işlemlerini sıraya al.
    // Aynı anda açılan iki sekme ikinci bir köy oluşturamaz.
    sqlx::query("SELECT pg_advisory_xact_lock(731001::bigint)")
        .execute(&mut *tx)
        .await?;

    let existing_id = sqlx::query_scalar::<_, Uuid>("SELECT id FROM villages WHERE owner_id = $1")
        .bind(user.id)
        .fetch_optional(&mut *tx)
        .await?;

    let village_id = if let Some(id) = existing_id {
        id
    } else {
        let position = sqlx::query_as::<_, (i32, i32)>(
            r#"
    SELECT gx.x, gy.y
    FROM generate_series(460, 540, 4) AS gx(x)
    CROSS JOIN generate_series(460, 540, 4) AS gy(y)
    WHERE NOT EXISTS (
        SELECT 1
        FROM villages v
        WHERE v.world_id = $1
          AND (
              (v.x - gx.x) * (v.x - gx.x)
              + (v.y - gy.y) * (v.y - gy.y)
          ) < 16
    )
    ORDER BY
        (gx.x - 500) * (gx.x - 500)
        + (gy.y - 500) * (gy.y - 500),
        gy.y,
        gx.x
    LIMIT 1
    "#,
        )
        .bind(world_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::BadRequest("Starting area is full"))?;

        sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO villages (
                id,
                owner_id,
                world_id,
                x,
                y
            )
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(user.id)
        .bind(world_id)
        .bind(position.0)
        .bind(position.1)
        .fetch_one(&mut *tx)
        .await?
    };

    // Başlangıç kaynakları villages tablosunun default'undan gelir.
    // Binalar da aynı transaction içinde oluşturulur.
    for kind in ["headquarters", "timber", "warehouse"] {
        sqlx::query(
            r#"
            INSERT INTO village_buildings (village_id, kind)
            VALUES ($1, $2)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(village_id)
        .bind(kind)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn rename_village(
    State(state): State<SharedState>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(input): Json<RenameRequest>,
) -> Result<StatusCode, AppError> {
    check_origin(&state, &headers)?;
    let user = current_user(&state, &jar).await?;

    let name = input.name.trim();

    if !(3..=32).contains(&name.chars().count()) || name.chars().any(char::is_control) {
        return Err(AppError::BadRequest(
            "Köy adı 3–32 karakter olmalı ve kontrol karakteri içermemeli.",
        ));
    }

    let result = sqlx::query("UPDATE villages SET name = $1 WHERE owner_id = $2")
        .bind(name)
        .bind(user.id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn start_upgrade(
    State(state): State<SharedState>,
    Path(kind): Path<String>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<(StatusCode, Json<Value>), AppError> {
    check_origin(&state, &headers)?;
    let user = current_user(&state, &jar).await?;

    if !matches!(kind.as_str(), "headquarters" | "timber" | "warehouse") {
        return Err(AppError::BadRequest("Geçersiz bina."));
    }

    let mut tx = state.db.begin().await?;

    // Kaynak harcaması ve kuyruk kontrolünü aynı köy kilidiyle koru.
    let village = sqlx::query_as::<_, Village>(
        r#"
        SELECT id, name, wood
        FROM villages
        WHERE owner_id = $1
        FOR UPDATE
        "#,
    )
    .bind(user.id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;

    let pending: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM building_upgrades
            WHERE village_id = $1
              AND completed_at IS NULL
        )
        "#,
    )
    .bind(village.id)
    .fetch_one(&mut *tx)
    .await?;

    if pending {
        return Err(AppError::BadRequest(
            "Köyünde zaten devam eden bir inşaat var.",
        ));
    }

    let level: i32 = sqlx::query_scalar(
        r#"
        SELECT level
        FROM village_buildings
        WHERE village_id = $1 AND kind = $2
        FOR UPDATE
        "#,
    )
    .bind(village.id)
    .bind(&kind)
    .fetch_one(&mut *tx)
    .await?;

    if level >= 20 {
        return Err(AppError::BadRequest("Bina en yüksek seviyede."));
    }

    let target_level = level + 1;
    let cost = i64::from(target_level) * 100;
    let duration_seconds = target_level * 15;

    if village.wood < cost {
        return Err(AppError::BadRequest("Yeterli odun yok."));
    }

    sqlx::query("UPDATE villages SET wood = wood - $1 WHERE id = $2")
        .bind(cost)
        .bind(village.id)
        .execute(&mut *tx)
        .await?;

    let job_id = Uuid::new_v4();

    let run_at: DateTime<Utc> = sqlx::query_scalar(
        r#"
        INSERT INTO scheduled_jobs (id, kind, payload, run_at)
        VALUES (
            $1,
            'building.upgrade.v1',
            '{}'::jsonb,
            NOW() + ($2::integer * INTERVAL '1 second')
        )
        RETURNING run_at
        "#,
    )
    .bind(job_id)
    .bind(duration_seconds)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO building_upgrades (
            job_id,
            village_id,
            building_kind,
            target_level
        )
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(job_id)
    .bind(village.id)
    .bind(&kind)
    .bind(target_level)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "job_id": job_id,
            "run_at": run_at,
            "target_level": target_level
        })),
    ))
}

// Worker endpoint'i tarafından mevcut transaction içinde çağrılır.
// İstemciden seviye, maliyet veya village_id kabul etmez.
pub async fn complete_upgrade(connection: &mut PgConnection, job_id: Uuid) -> Result<(), AppError> {
    let upgrade = sqlx::query_as::<_, (Uuid, String, i32)>(
        r#"
        SELECT village_id, building_kind, target_level
        FROM building_upgrades
        WHERE job_id = $1
          AND completed_at IS NULL
        "#,
    )
    .bind(job_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(AppError::NotFound)?;

    let (village_id, building_kind, target_level) = upgrade;

    sqlx::query("SELECT id FROM villages WHERE id = $1 FOR UPDATE")
        .bind(village_id)
        .fetch_one(&mut *connection)
        .await?;

    let result = sqlx::query(
        r#"
        UPDATE village_buildings
        SET level = $1
        WHERE village_id = $2
          AND kind = $3
          AND level = $1 - 1
        "#,
    )
    .bind(target_level)
    .bind(village_id)
    .bind(building_kind)
    .execute(&mut *connection)
    .await?;

    if result.rows_affected() != 1 {
        return Err(AppError::Internal(anyhow::anyhow!(
            "Unexpected building level for job {job_id}"
        )));
    }

    sqlx::query(
        r#"
        UPDATE building_upgrades
        SET completed_at = NOW()
        WHERE job_id = $1
        "#,
    )
    .bind(job_id)
    .execute(&mut *connection)
    .await?;

    Ok(())
}
