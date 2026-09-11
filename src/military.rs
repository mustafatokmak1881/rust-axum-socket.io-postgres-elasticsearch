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
    economy,
    error::AppError,
    security::hash_token,
    state::SharedState,
};

// Referans hareket değeri: 18 dakika / kare.
// TEST için 540 kat hızlandırılmıştır: yaklaşık 2 saniye / kare.
// Normal referans hareket için bunu 1.0 yap.
const MOVEMENT_SPEED: f64 = 540.0;
const SPEAR_SECONDS_PER_TILE: f64 = 18.0 * 60.0;

/// Klanlar.org mızrakçı taşıma kapasitesi.
const SPEAR_CARRY_CAPACITY: i64 = 25;

/// Yeni köy başlangıç ordusu (erken yağma/savunma için).
pub const STARTING_SPEARS: i64 = 100;

#[derive(Deserialize)]
pub struct AttackRequest {
    pub request_id: Uuid,
    pub target_id: Uuid,
    pub spears: i64,
}

#[derive(Deserialize)]
pub struct RecruitRequest {
    pub count: i64,
}

#[derive(FromRow)]
struct Recruit {
    job_id: Uuid,
    village_id: Uuid,
    unit_kind: String,
    count: i64,
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
    loot_wood: Option<i64>,
    loot_clay: Option<i64>,
    loot_iron: Option<i64>,

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

/// Oyuncunun köydeki askerleri, giden emirleri ve gelen saldırıları.
/// Başka oyuncuların köydeki (evde bekleyen) askerlerini paylaşmaz;
/// gelen saldırılarda yalnızca yoldaki birlik sayısı görünür.
pub async fn bootstrap(
    State(state): State<SharedState>,
    jar: CookieJar,
) -> Result<impl axum::response::IntoResponse, AppError> {
    let user = current_user(&state, &jar).await?;

    let mut tx = state.db.begin().await?;

    let army = sqlx::query_as::<_, (Uuid, i64)>(
        r#"
        SELECT v.id, a.spears
        FROM villages v
        JOIN village_armies a ON a.village_id = v.id
        WHERE v.owner_id = $1
        FOR UPDATE OF v
        "#,
    )
    .bind(user.id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;

    let village_id = army.0;
    let home_spears = army.1;

    let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *tx)
        .await?;

    let economy_snapshot = economy::settle(&mut *tx, village_id, now).await?;

    let barracks_level = building_level(&mut *tx, village_id, "barracks").await?;
    let farm_level = building_level(&mut *tx, village_id, "farm").await?;
    let farm_capacity = economy::farm_capacity(farm_level)?;

    let away_spears = away_spears(&mut *tx, village_id).await?;
    let training_spears = training_spears(&mut *tx, village_id).await?;
    let population_used =
        home_spears + away_spears + training_spears;
    let farm_free = (farm_capacity - population_used).max(0);

    let recruit = sqlx::query_as::<_, (Uuid, String, i64, DateTime<Utc>)>(
        r#"
        SELECT r.job_id, r.unit_kind, r.count, j.run_at
        FROM army_recruits r
        JOIN scheduled_jobs j ON j.id = r.job_id
        WHERE r.village_id = $1
          AND r.completed_at IS NULL
        "#,
    )
    .bind(village_id)
    .fetch_optional(&mut *tx)
    .await?;

    let attacks: Vec<Value> = sqlx::query_scalar(
        r#"
        SELECT jsonb_build_object(
            'id', a.id,
            'target_name', v.name,
            'target_x', v.x,
            'target_y', v.y,
            'sent_spears', a.sent_spears,
            'surviving_spears', a.surviving_spears,
            'loot_wood', a.loot_wood,
            'loot_clay', a.loot_clay,
            'loot_iron', a.loot_iron,
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
    .fetch_all(&mut *tx)
    .await?;

    // Hedefi oyuncunun köyü olan ve henüz çarpışmamış saldırılar.
    let incoming: Vec<Value> = sqlx::query_scalar(
        r#"
        SELECT jsonb_build_object(
            'id', a.id,
            'source_name', s.name,
            'source_x', s.x,
            'source_y', s.y,
            'sent_spears', a.sent_spears,
            'arrives_at', a.arrives_at,
            'departed_at', a.departed_at
        )
        FROM army_attacks a
        JOIN villages t ON t.id = a.target_id
        JOIN villages s ON s.id = a.source_id
        WHERE t.owner_id = $1
          AND a.status = 'outbound'
        ORDER BY a.arrives_at ASC, a.id ASC
        LIMIT 50
        "#,
    )
    .bind(user.id)
    .fetch_all(&mut *tx)
    .await?;

    tx.commit().await?;

    let recruit_json = recruit.map(|(job_id, unit_kind, count, finishes_at)| {
        json!({
            "job_id": job_id,
            "unit": unit_kind,
            "count": count,
            "finishes_at": finishes_at
        })
    });

    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "source_id": village_id,
            "spears": home_spears,
            "army": {
                "home": {
                    "spear": home_spears
                },
                "away": {
                    "spear": away_spears
                },
                "training": {
                    "spear": training_spears
                },
                "total": {
                    "spear": home_spears + away_spears + training_spears
                }
            },
            "barracks_level": barracks_level,
            "farm": {
                "level": farm_level,
                "capacity": farm_capacity,
                "used": population_used,
                "free": farm_free
            },
            "units": {
                "spear": {
                    "name": "Mızrakçı",
                    "wood_cost": economy::SPEAR_WOOD_COST,
                    "clay_cost": economy::SPEAR_CLAY_COST,
                    "iron_cost": economy::SPEAR_IRON_COST,
                    "population": economy::SPEAR_POPULATION,
                    "carry": SPEAR_CARRY_CAPACITY,
                    "requires_barracks": 1,
                    "available": barracks_level >= 1
                }
            },
            "recruit": recruit_json,
            "wood": economy_snapshot.wood,
            "clay": economy_snapshot.clay,
            "iron": economy_snapshot.iron,
            "seconds_per_tile":
                SPEAR_SECONDS_PER_TILE / MOVEMENT_SPEED,
            "incoming": incoming,
            "attacks": attacks
        })),
    ))
}

async fn building_level(
    connection: &mut PgConnection,
    village_id: Uuid,
    kind: &str,
) -> Result<i32, AppError> {
    Ok(sqlx::query_scalar::<_, i32>(
        r#"
        SELECT level
        FROM village_buildings
        WHERE village_id = $1 AND kind = $2
        "#,
    )
    .bind(village_id)
    .bind(kind)
    .fetch_optional(&mut *connection)
    .await?
    .unwrap_or(0))
}

async fn away_spears(
    connection: &mut PgConnection,
    village_id: Uuid,
) -> Result<i64, AppError> {
    Ok(sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COALESCE(SUM(
            CASE
                WHEN status = 'outbound' THEN sent_spears
                WHEN status = 'returning' THEN surviving_spears
                ELSE 0
            END
        ), 0)::bigint
        FROM army_attacks
        WHERE source_id = $1
          AND status IN ('outbound', 'returning')
        "#,
    )
    .bind(village_id)
    .fetch_one(&mut *connection)
    .await?)
}

async fn training_spears(
    connection: &mut PgConnection,
    village_id: Uuid,
) -> Result<i64, AppError> {
    Ok(sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COALESCE(SUM(count), 0)::bigint
        FROM army_recruits
        WHERE village_id = $1
          AND completed_at IS NULL
          AND unit_kind = 'spear'
        "#,
    )
    .bind(village_id)
    .fetch_one(&mut *connection)
    .await?)
}

/// Kışlada mızrakçı eğitir. Aynı anda tek eğitim kuyruğu.
pub async fn start_recruit(
    State(state): State<SharedState>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(input): Json<RecruitRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    check_origin(&state, &headers)?;
    let user = current_user(&state, &jar).await?;

    if !(1..=10_000).contains(&input.count) {
        return Err(AppError::BadRequest(
            "Eğitilecek birlik sayısı 1–10.000 arasında olmalı.",
        ));
    }

    let mut tx = state.db.begin().await?;

    let village = sqlx::query_as::<_, (Uuid,)>(
        r#"
        SELECT id
        FROM villages
        WHERE owner_id = $1
        FOR UPDATE
        "#,
    )
    .bind(user.id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;

    let village_id = village.0;

    let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *tx)
        .await?;

    let resources = economy::settle(&mut *tx, village_id, now).await?;

    let barracks_level = building_level(&mut *tx, village_id, "barracks").await?;

    if barracks_level < 1 {
        return Err(AppError::BadRequest(
            "Mızrakçı eğitmek için kışla gerekli (Bey otağı 3).",
        ));
    }

    let pending: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM army_recruits
            WHERE village_id = $1
              AND completed_at IS NULL
        )
        "#,
    )
    .bind(village_id)
    .fetch_one(&mut *tx)
    .await?;

    if pending {
        return Err(AppError::BadRequest(
            "Köyünde zaten devam eden bir eğitim var.",
        ));
    }

    let farm_level = building_level(&mut *tx, village_id, "farm").await?;
    let farm_capacity = economy::farm_capacity(farm_level)?;

    let home_spears: i64 = sqlx::query_scalar(
        r#"
        SELECT spears
        FROM village_armies
        WHERE village_id = $1
        FOR UPDATE
        "#,
    )
    .bind(village_id)
    .fetch_one(&mut *tx)
    .await?;

    let away = away_spears(&mut *tx, village_id).await?;
    let population_needed = input.count.saturating_mul(economy::SPEAR_POPULATION);
    let population_used = home_spears + away;
    let farm_free = (farm_capacity - population_used).max(0);

    if population_needed > farm_free {
        return Err(AppError::BadRequest(
            "Çiftlikte yeterli nüfus alanı yok.",
        ));
    }

    let cost = economy::spear_cost(input.count)?;

    if resources.wood < cost.wood {
        return Err(AppError::BadRequest("Yeterli odun yok."));
    }
    if resources.clay < cost.clay {
        return Err(AppError::BadRequest("Yeterli kil yok."));
    }
    if resources.iron < cost.iron {
        return Err(AppError::BadRequest("Yeterli demir yok."));
    }

    let duration_seconds =
        economy::spear_recruit_seconds(input.count, barracks_level)?;

    sqlx::query(
        r#"
        UPDATE villages
        SET wood = wood - $2,
            clay = clay - $3,
            iron = iron - $4
        WHERE id = $1
        "#,
    )
    .bind(village_id)
    .bind(cost.wood)
    .bind(cost.clay)
    .bind(cost.iron)
    .execute(&mut *tx)
    .await?;

    let job_id = Uuid::new_v4();

    let finishes_at: DateTime<Utc> = sqlx::query_scalar(
        r#"
        INSERT INTO scheduled_jobs (id, kind, payload, run_at)
        VALUES (
            $1,
            'army.recruit.complete.v1',
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
        INSERT INTO army_recruits (job_id, village_id, unit_kind, count)
        VALUES ($1, $2, 'spear', $3)
        "#,
    )
    .bind(job_id)
    .bind(village_id)
    .bind(input.count)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "job_id": job_id,
            "unit": "spear",
            "count": input.count,
            "finishes_at": finishes_at,
            "wood_cost": cost.wood,
            "clay_cost": cost.clay,
            "iron_cost": cost.iron,
            "duration_seconds": duration_seconds
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
    if let Some(recruit) = sqlx::query_as::<_, Recruit>(
        r#"
        SELECT job_id, village_id, unit_kind, count
        FROM army_recruits
        WHERE job_id = $1
          AND completed_at IS NULL
        FOR UPDATE
        "#,
    )
    .bind(job_id)
    .fetch_optional(&mut *connection)
    .await?
    {
        return complete_recruit(connection, recruit).await;
    }

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

        // Ganimet yalnızca saldırı kazanınca (sağ kalan > 0).
        // Üç hammadde orantılı alınır; gizli depo her birinden korunur.
        let loot = if survivors > 0 {
            let target_economy =
                economy::settle(&mut *connection, attack.target_id, now)
                    .await?;

            let hiding_level = sqlx::query_scalar::<_, i32>(
                r#"
                SELECT level
                FROM village_buildings
                WHERE village_id = $1 AND kind = 'hiding_place'
                "#,
            )
            .bind(attack.target_id)
            .fetch_optional(&mut *connection)
            .await?
            .unwrap_or(0);

            let hidden = economy::hiding_capacity(hiding_level)?;
            let capacity = survivors.saturating_mul(SPEAR_CARRY_CAPACITY);
            let loot = economy::split_loot(
                capacity,
                (target_economy.wood - hidden).max(0),
                (target_economy.clay - hidden).max(0),
                (target_economy.iron - hidden).max(0),
            );

            if loot.wood > 0 || loot.clay > 0 || loot.iron > 0 {
                sqlx::query(
                    r#"
                    UPDATE villages
                    SET wood = $2,
                        clay = $3,
                        iron = $4
                    WHERE id = $1
                    "#,
                )
                .bind(attack.target_id)
                .bind(target_economy.wood - loot.wood)
                .bind(target_economy.clay - loot.clay)
                .bind(target_economy.iron - loot.iron)
                .execute(&mut *connection)
                .await?;
            }

            loot
        } else {
            economy::ResourceCost {
                wood: 0,
                clay: 0,
                iron: 0,
            }
        };

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
                loot_wood = $5,
                loot_clay = $6,
                loot_iron = $7,
                resolved_at = $8,
                return_job_id = $9,
                returns_at = $10,
                status = $11
            WHERE id = $1
            "#,
        )
        .bind(attack.id)
        .bind(survivors)
        .bind(defenders)
        .bind(defenders_after)
        .bind(loot.wood)
        .bind(loot.clay)
        .bind(loot.iron)
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

        let loot_wood = attack.loot_wood.unwrap_or(0);
        let loot_clay = attack.loot_clay.unwrap_or(0);
        let loot_iron = attack.loot_iron.unwrap_or(0);

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

        if loot_wood > 0 || loot_clay > 0 || loot_iron > 0 {
            economy::settle(&mut *connection, attack.source_id, now).await?;

            sqlx::query(
                r#"
                UPDATE villages
                SET wood = wood + $2,
                    clay = clay + $3,
                    iron = iron + $4
                WHERE id = $1
                "#,
            )
            .bind(attack.source_id)
            .bind(loot_wood)
            .bind(loot_clay)
            .bind(loot_iron)
            .execute(&mut *connection)
            .await?;
        }

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

async fn complete_recruit(
    connection: &mut PgConnection,
    recruit: Recruit,
) -> Result<(), AppError> {
    let run_at: DateTime<Utc> = sqlx::query_scalar(
        r#"
        SELECT run_at
        FROM scheduled_jobs
        WHERE id = $1
        "#,
    )
    .bind(recruit.job_id)
    .fetch_one(&mut *connection)
    .await?;

    let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *connection)
        .await?;

    if now < run_at {
        return Err(AppError::BadRequest("Job is not due yet"));
    }

    sqlx::query("SELECT id FROM villages WHERE id = $1 FOR UPDATE")
        .bind(recruit.village_id)
        .fetch_one(&mut *connection)
        .await?;

    if recruit.unit_kind != "spear" {
        return Err(AppError::Internal(anyhow::anyhow!(
            "Unknown recruit unit {}",
            recruit.unit_kind
        )));
    }

    sqlx::query(
        r#"
        UPDATE village_armies
        SET spears = spears + $2
        WHERE village_id = $1
        "#,
    )
    .bind(recruit.village_id)
    .bind(recruit.count)
    .execute(&mut *connection)
    .await?;

    sqlx::query(
        r#"
        UPDATE army_recruits
        SET completed_at = $2
        WHERE job_id = $1
        "#,
    )
    .bind(recruit.job_id)
    .bind(now)
    .execute(&mut *connection)
    .await?;

    Ok(())
}