use crate::economy;

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

#[derive(Serialize)]
struct BuildingOffer {
    kind: String,
    level: i32,
    max_level: i32,

    cost_wood: Option<i64>,
    duration_seconds: Option<i32>,

    production_per_hour: Option<i64>,
    next_production_per_hour: Option<i64>,

    requirements: Vec<economy::Requirement>,
    can_upgrade: bool,
    blocked_reason: Option<String>,
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

/// İlk köy adı hesap e-postasının yerel kısmından türetilir.
fn village_name_from_account(email: &str) -> String {
    let local = email.split('@').next().unwrap_or(email);
    let cleaned: String = local
        .chars()
        .filter(|character| !character.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(32)
        .collect();

    let char_count = cleaned.chars().count();

    if char_count >= 3 {
        cleaned
    } else if cleaned.is_empty() {
        "Yeni Oba".to_owned()
    } else {
        format!("{cleaned}oba").chars().take(32).collect()
    }
}

pub async fn bootstrap(
    State(state): State<SharedState>,
    jar: CookieJar,
) -> Result<Json<Value>, AppError> {
    let user = current_user(&state, &jar).await?;
    let mut tx = state.db.begin().await?;

    let mut village = sqlx::query_as::<_, Village>(
        r#"
        SELECT id, name, wood
        FROM villages
        WHERE owner_id = $1
        FOR UPDATE
        "#,
    )
    .bind(user.id)
    .fetch_optional(&mut *tx)
    .await?;

    // Köy kilidi alındıktan sonra zamanı al.
    let server_time: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *tx)
        .await?;

    let mut buildings: Vec<Building> = Vec::new();
    let mut upgrades: Vec<Upgrade> = Vec::new();
    let mut offers: Vec<BuildingOffer> = Vec::new();
    let mut economy_snapshot = None;

    if let Some(village) = village.as_mut() {
        let resources = economy::settle(&mut *tx, village.id, server_time).await?;

        village.wood = resources.wood;
        economy_snapshot = Some(resources);

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

        let pending = upgrades.iter().any(|u| u.completed_at.is_none());

        for &kind in economy::BUILDING_KINDS {
            let level = buildings
                .iter()
                .find(|building| building.kind == kind)
                .map(|building| building.level)
                .unwrap_or(0);

            let max_level = economy::max_level(kind).ok_or_else(|| {
                AppError::Internal(anyhow::anyhow!("Unknown building kind: {kind}"))
            })?;

            let maxed = level >= max_level;
            let target = level + 1;

            let cost = if maxed {
                None
            } else {
                Some(economy::upgrade_cost(target))
            };

            let seconds = if maxed {
                None
            } else {
                Some(economy::upgrade_seconds(target))
            };

            let requirements =
                economy::requirement_status(&mut *tx, village.id, kind).await?;

            let missing = requirements.iter().find(|r| !r.met);

            let blocked_reason = if maxed {
                Some("En yüksek seviye".to_owned())
            } else if let Some(requirement) = missing {
                Some(format!(
                    "{} seviye {} gerekli",
                    requirement.name, requirement.required_level,
                ))
            } else if pending {
                Some("İnşaat sürüyor".to_owned())
            } else if village.wood < cost.unwrap_or(0) {
                Some("Odun yetersiz".to_owned())
            } else {
                None
            };

            let production = if kind == "timber" {
                Some(economy::timber_production(level)?)
            } else {
                None
            };

            let next_production = if kind == "timber" && !maxed {
                Some(economy::timber_production(target)?)
            } else {
                None
            };

            offers.push(BuildingOffer {
                kind: kind.to_owned(),
                level,
                max_level,
                cost_wood: cost,
                duration_seconds: seconds,
                production_per_hour: production,
                next_production_per_hour: next_production,
                requirements,
                can_upgrade: blocked_reason.is_none(),
                blocked_reason,
            });
        }
    }

    tx.commit().await?;

    Ok(Json(json!({
        "user": user,
        "village": village,
        "buildings": buildings,
        "upgrades": upgrades,
        "offers": offers,
        "economy": economy_snapshot,
        "server_time": server_time,

        // Eski istemci alanlarını geçiş için koruyoruz.
        // Yeni frontend teklifler için offers alanını kullanacak.
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
                y,
                name
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(user.id)
        .bind(world_id)
        .bind(position.0)
        .bind(position.1)
        .bind(village_name_from_account(&user.email))
        .fetch_one(&mut *tx)
        .await?
    };

    // Başlangıç kaynakları villages tablosunun default'undan gelir.
    // Binalar da aynı transaction içinde oluşturulur.
    for &kind in economy::BUILDING_KINDS {
        sqlx::query(
            r#"
            INSERT INTO village_buildings (village_id, kind, level)
            VALUES ($1, $2, $3)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(village_id)
        .bind(kind)
        .bind(economy::starting_level(kind))
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query(
        r#"
    INSERT INTO village_armies (village_id, spears)
    VALUES ($1, 50)
    ON CONFLICT (village_id) DO NOTHING
    "#,
    )
    .bind(village_id)
    .execute(&mut *tx)
    .await?;

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

    if !economy::is_known_building(&kind) {
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

    let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *tx)
        .await?;

    let resources = economy::settle(&mut *tx, village.id, now).await?;

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

    let max_level = economy::max_level(&kind).ok_or(AppError::BadRequest("Geçersiz bina."))?;

    if level >= max_level {
        return Err(AppError::BadRequest("Bina en yüksek seviyede."));
    }

    economy::ensure_requirements(&mut *tx, village.id, &kind).await?;

    let target_level = level + 1;
    let cost = economy::upgrade_cost(target_level);
    let duration_seconds = economy::upgrade_seconds(target_level);

    if resources.wood < cost {
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
            clock_timestamp() + ($2::integer * INTERVAL '1 second')
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

pub async fn complete_upgrade(connection: &mut PgConnection, job_id: Uuid) -> Result<(), AppError> {
    let upgrade = sqlx::query_as::<_, (Uuid, String, i32, DateTime<Utc>)>(
        r#"
        SELECT
            u.village_id,
            u.building_kind,
            u.target_level,
            j.run_at
        FROM building_upgrades u
        JOIN scheduled_jobs j ON j.id = u.job_id
        WHERE u.job_id = $1
          AND u.completed_at IS NULL
        "#,
    )
    .bind(job_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(AppError::NotFound)?;

    let (village_id, building_kind, target_level, run_at) = upgrade;

    sqlx::query("SELECT id FROM villages WHERE id = $1 FOR UPDATE")
        .bind(village_id)
        .fetch_one(&mut *connection)
        .await?;

    let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *connection)
        .await?;

    if now < run_at {
        return Err(AppError::BadRequest("Job is not due yet"));
    }

    // 1. İnşaatın hedef bitişine kadar eski seviye üretimi.
    economy::settle(&mut *connection, village_id, run_at).await?;

    // 2. Bina seviyesini yükselt.
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
    .bind(&building_kind)
    .execute(&mut *connection)
    .await?;

    if result.rows_affected() != 1 {
        return Err(AppError::Internal(anyhow::anyhow!(
            "Unexpected building level for job {job_id}"
        )));
    }

    // 3. Bekleyen üretim sınırını kaldır.
    sqlx::query(
        r#"
        UPDATE building_upgrades
        SET completed_at = $2
        WHERE job_id = $1
        "#,
    )
    .bind(job_id)
    .bind(now)
    .execute(&mut *connection)
    .await?;

    // 4. Worker geç çalıştıysa run_at -> now arası yeni hızla üret.
    economy::settle(&mut *connection, village_id, now).await?;

    Ok(())
}
