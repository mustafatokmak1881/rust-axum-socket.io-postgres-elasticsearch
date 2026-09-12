use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgConnection};
use uuid::Uuid;

use crate::error::AppError;

const MICROS_PER_HOUR: i128 = 3_600_000_000;

/// Klanlar.org / Tribal Wars bina kataloğu (dünya ayarına bağlı
/// tapınak ve gözetleme kulesi hariç).
pub const BUILDING_KINDS: &[&str] = &[
    "headquarters",
    "barracks",
    "stable",
    "workshop",
    "academy",
    "smithy",
    "rally_point",
    "statue",
    "market",
    "timber",
    "clay",
    "iron",
    "farm",
    "warehouse",
    "hiding_place",
    "wall",
];

/// Yeni köyde seviye 1 başlayan binalar (Klanlar.org / modern TW).
/// Oduncu, kil ocağı ve demir madeni oyuncu tarafından inşa edilir.
pub const STARTING_BUILDINGS: &[(&str, i32)] = &[
    ("headquarters", 1),
    ("rally_point", 1),
    ("farm", 1),
    ("warehouse", 1),
    ("hiding_place", 1),
];

// 1× dünya: oduncu / kil ocağı / demir madeni aynı üretim eğrisi.
const RESOURCE_PRODUCTION: [i64; 31] = [
    0, 30, 35, 41, 47, 55, 64, 74, 86, 100, 117, 136, 158, 184, 214, 249, 289,
    337, 391, 455, 530, 616, 717, 833, 969, 1127, 1311, 1525, 1774, 2063, 2400,
];

pub fn resource_production(level: i32) -> Result<i64, AppError> {
    if !(0..=30).contains(&level) {
        return Err(AppError::Internal(anyhow::anyhow!(
            "Invalid resource building level: {level}"
        )));
    }

    Ok(RESOURCE_PRODUCTION[level as usize])
}

pub fn timber_production(level: i32) -> Result<i64, AppError> {
    resource_production(level)
}

pub fn clay_production(level: i32) -> Result<i64, AppError> {
    resource_production(level)
}

pub fn iron_production(level: i32) -> Result<i64, AppError> {
    resource_production(level)
}

pub fn is_known_building(kind: &str) -> bool {
    BUILDING_KINDS.contains(&kind)
}

pub fn max_level(kind: &str) -> Option<i32> {
    Some(match kind {
        "headquarters" | "timber" | "clay" | "iron" | "farm" | "warehouse" => 30,
        "barracks" | "market" => 25,
        "stable" | "smithy" | "wall" => 20,
        "workshop" => 15,
        "hiding_place" => 10,
        "academy" | "rally_point" | "statue" => 1,
        _ => return None,
    })
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct ResourceCost {
    pub wood: i64,
    pub clay: i64,
    pub iron: i64,
}

// Geliştirme maliyetleri (henüz tam TW tabloları değil).
pub fn upgrade_cost(target_level: i32) -> ResourceCost {
    let n = i64::from(target_level);
    ResourceCost {
        wood: n * 100,
        clay: n * 80,
        iron: n * 70,
    }
}

pub fn upgrade_seconds(target_level: i32) -> i32 {
    target_level * 15
}

fn requirements(kind: &str) -> &'static [(&'static str, i32)] {
    match kind {
        "barracks" => &[("headquarters", 3)],
        "market" => &[("headquarters", 3), ("warehouse", 2)],
        "smithy" => &[("headquarters", 5), ("barracks", 1)],
        "wall" => &[("barracks", 1)],
        "stable" => &[("headquarters", 10), ("barracks", 5), ("smithy", 5)],
        "workshop" => &[("headquarters", 10), ("smithy", 10)],
        "academy" => &[("headquarters", 20), ("smithy", 20), ("market", 10)],
        _ => &[],
    }
}

pub fn building_name(kind: &str) -> &'static str {
    match kind {
        "headquarters" => "Bey otağı",
        "barracks" => "Kışla",
        "stable" => "Ahır",
        "workshop" => "Atölye",
        "academy" => "Akademi",
        "smithy" => "Demirci",
        "rally_point" => "İçtima meydanı",
        "statue" => "Heykel",
        "market" => "Pazar",
        "timber" => "Oduncu",
        "clay" => "Kil ocağı",
        "iron" => "Demir madeni",
        "farm" => "Çiftlik",
        "warehouse" => "Ambar",
        "hiding_place" => "Gizli depo",
        "wall" => "Duvar",
        _ => "Bilinmeyen bina",
    }
}

pub fn starting_level(kind: &str) -> i32 {
    STARTING_BUILDINGS
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, level)| *level)
        .unwrap_or(0)
}

/// Klanlar.org gizli depo kapasitesi (1× dünya).
const HIDING_CAPACITY: [i64; 11] = [
    0, 150, 200, 274, 354, 456, 584, 747, 956, 1222, 1560,
];

pub fn hiding_capacity(level: i32) -> Result<i64, AppError> {
    if !(0..=10).contains(&level) {
        return Err(AppError::Internal(anyhow::anyhow!(
            "Invalid hiding place level: {level}"
        )));
    }

    Ok(HIDING_CAPACITY[level as usize])
}

/// Klanlar.org 1× dünya çiftlik kapasitesi (yaklaşık).
const FARM_CAPACITY: [i64; 31] = [
    0, 240, 281, 329, 386, 452, 530, 622, 729, 854, 1002, 1174, 1376, 1613,
    1891, 2216, 2598, 3045, 3569, 4183, 4904, 5748, 6737, 7896, 9255, 10848,
    12715, 14904, 17469, 20476, 24000,
];

pub fn farm_capacity(level: i32) -> Result<i64, AppError> {
    if !(0..=30).contains(&level) {
        return Err(AppError::Internal(anyhow::anyhow!(
            "Invalid farm level: {level}"
        )));
    }

    Ok(FARM_CAPACITY[level as usize])
}

/// Mızrakçı: Klanlar.org 50 odun / 30 kil / 10 demir.
pub const SPEAR_WOOD_COST: i64 = 50;
pub const SPEAR_CLAY_COST: i64 = 30;
pub const SPEAR_IRON_COST: i64 = 10;
pub const SPEAR_POPULATION: i64 = 1;

pub fn spear_cost(count: i64) -> Result<ResourceCost, AppError> {
    Ok(ResourceCost {
        wood: count
            .checked_mul(SPEAR_WOOD_COST)
            .ok_or(AppError::BadRequest("Maliyet taşması."))?,
        clay: count
            .checked_mul(SPEAR_CLAY_COST)
            .ok_or(AppError::BadRequest("Maliyet taşması."))?,
        iron: count
            .checked_mul(SPEAR_IRON_COST)
            .ok_or(AppError::BadRequest("Maliyet taşması."))?,
    })
}

pub fn spear_recruit_seconds(count: i64, barracks_level: i32) -> Result<i32, AppError> {
    if count < 1 || !(1..=25).contains(&barracks_level) {
        return Err(AppError::BadRequest("Geçersiz eğitim parametresi."));
    }

    let per_unit = (20.0 * 0.95_f64.powi(barracks_level - 1))
        .ceil()
        .max(1.0);

    let total = (count as f64 * per_unit).ceil();

    i32::try_from(total as i64).map_err(|_| AppError::BadRequest("Eğitim süresi çok uzun."))
}

/// Taşıma kapasitesini mevcut hammaddelere orantılı dağıtır.
pub fn split_loot(
    capacity: i64,
    available_wood: i64,
    available_clay: i64,
    available_iron: i64,
) -> ResourceCost {
    let wood = available_wood.max(0);
    let clay = available_clay.max(0);
    let iron = available_iron.max(0);
    let total = wood + clay + iron;

    if capacity <= 0 || total <= 0 {
        return ResourceCost {
            wood: 0,
            clay: 0,
            iron: 0,
        };
    }

    let take = capacity.min(total);
    let mut loot_wood = take * wood / total;
    let mut loot_clay = take * clay / total;
    let mut loot_iron = take * iron / total;
    let mut remaining = take - loot_wood - loot_clay - loot_iron;

    // Yuvarlama artığını doldurulabilir hammaddelere ver.
    while remaining > 0 {
        if loot_wood < wood {
            loot_wood += 1;
            remaining -= 1;
            if remaining == 0 {
                break;
            }
        }
        if loot_clay < clay {
            loot_clay += 1;
            remaining -= 1;
            if remaining == 0 {
                break;
            }
        }
        if loot_iron < iron {
            loot_iron += 1;
            remaining -= 1;
            if remaining == 0 {
                break;
            }
        }
        if loot_wood >= wood && loot_clay >= clay && loot_iron >= iron {
            break;
        }
    }

    ResourceCost {
        wood: loot_wood,
        clay: loot_clay,
        iron: loot_iron,
    }
}

#[derive(Serialize)]
pub struct Requirement {
    pub kind: String,
    pub name: String,
    pub required_level: i32,
    pub current_level: i32,
    pub met: bool,
}

pub async fn requirement_status(
    connection: &mut PgConnection,
    village_id: Uuid,
    kind: &str,
) -> Result<Vec<Requirement>, AppError> {
    let mut result = Vec::new();

    for &(required_kind, required_level) in requirements(kind) {
        let current_level = sqlx::query_scalar::<_, i32>(
            r#"
            SELECT level
            FROM village_buildings
            WHERE village_id = $1 AND kind = $2
            "#,
        )
        .bind(village_id)
        .bind(required_kind)
        .fetch_optional(&mut *connection)
        .await?
        .unwrap_or(0);

        result.push(Requirement {
            kind: required_kind.to_owned(),
            name: building_name(required_kind).to_owned(),
            required_level,
            current_level,
            met: current_level >= required_level,
        });
    }

    Ok(result)
}

pub async fn ensure_requirements(
    connection: &mut PgConnection,
    village_id: Uuid,
    kind: &str,
) -> Result<(), AppError> {
    let requirements = requirement_status(connection, village_id, kind).await?;

    if requirements.iter().any(|requirement| !requirement.met) {
        return Err(AppError::BadRequest(
            "Bu bina için gerekli diğer bina seviyeleri sağlanmıyor.",
        ));
    }

    Ok(())
}

#[derive(FromRow)]
struct ResourceRow {
    wood: i64,
    clay: i64,
    iron: i64,
    wood_remainder: i64,
    clay_remainder: i64,
    iron_remainder: i64,
    resources_updated_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct EconomySnapshot {
    pub wood: i64,
    pub clay: i64,
    pub iron: i64,
    pub wood_remainder: i64,
    pub clay_remainder: i64,
    pub iron_remainder: i64,
    pub wood_per_hour: i64,
    pub clay_per_hour: i64,
    pub iron_per_hour: i64,
    pub resources_updated_at: DateTime<Utc>,
    pub production_valid_until: Option<DateTime<Utc>>,
}

fn accrue(balance: i64, remainder: i64, rate: i64, elapsed_micros: i64) -> Result<(i64, i64), AppError> {
    let accumulated =
        i128::from(remainder) + i128::from(elapsed_micros) * i128::from(rate);

    let produced = accumulated / MICROS_PER_HOUR;
    let next_remainder = (accumulated % MICROS_PER_HOUR) as i64;

    let next_balance = i64::try_from(i128::from(balance) + produced).map_err(|_| {
        AppError::Internal(anyhow::anyhow!("Resource balance overflow"))
    })?;

    Ok((next_balance, next_remainder))
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

/// Mevcut transaction içinde çağrılır.
///
/// Köy satırını kilitler; üç hammaddenin üretimini ve kesirli payını kaydeder.
pub async fn settle(
    connection: &mut PgConnection,
    village_id: Uuid,
    requested_at: DateTime<Utc>,
) -> Result<EconomySnapshot, AppError> {
    let row = sqlx::query_as::<_, ResourceRow>(
        r#"
        SELECT
            wood, clay, iron,
            wood_remainder, clay_remainder, iron_remainder,
            resources_updated_at
        FROM villages
        WHERE id = $1
        FOR UPDATE
        "#,
    )
    .bind(village_id)
    .fetch_one(&mut *connection)
    .await?;

    let timber_level = building_level(connection, village_id, "timber").await?;
    let clay_level = building_level(connection, village_id, "clay").await?;
    let iron_level = building_level(connection, village_id, "iron").await?;

    let wood_per_hour = timber_production(timber_level)?;
    let clay_per_hour = clay_production(clay_level)?;
    let iron_per_hour = iron_production(iron_level)?;

    let boundary = sqlx::query_scalar::<_, DateTime<Utc>>(
        r#"
        SELECT j.run_at
        FROM building_upgrades u
        JOIN scheduled_jobs j ON j.id = u.job_id
        WHERE u.village_id = $1
          AND u.completed_at IS NULL
        ORDER BY j.run_at
        LIMIT 1
        "#,
    )
    .bind(village_id)
    .fetch_optional(&mut *connection)
    .await?;

    let mut effective_at = requested_at;

    if let Some(run_at) = boundary.as_ref() {
        if *run_at < effective_at {
            effective_at = *run_at;
        }
    }

    if effective_at < row.resources_updated_at {
        effective_at = row.resources_updated_at;
    }

    let elapsed_micros = effective_at
        .signed_duration_since(row.resources_updated_at)
        .num_microseconds()
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("Resource time interval is too large"))
        })?;

    let (new_wood, wood_remainder) =
        accrue(row.wood, row.wood_remainder, wood_per_hour, elapsed_micros)?;
    let (new_clay, clay_remainder) =
        accrue(row.clay, row.clay_remainder, clay_per_hour, elapsed_micros)?;
    let (new_iron, iron_remainder) =
        accrue(row.iron, row.iron_remainder, iron_per_hour, elapsed_micros)?;

    sqlx::query(
        r#"
        UPDATE villages
        SET wood = $2,
            clay = $3,
            iron = $4,
            wood_remainder = $5,
            clay_remainder = $6,
            iron_remainder = $7,
            resources_updated_at = $8
        WHERE id = $1
        "#,
    )
    .bind(village_id)
    .bind(new_wood)
    .bind(new_clay)
    .bind(new_iron)
    .bind(wood_remainder)
    .bind(clay_remainder)
    .bind(iron_remainder)
    .bind(effective_at)
    .execute(&mut *connection)
    .await?;

    Ok(EconomySnapshot {
        wood: new_wood,
        clay: new_clay,
        iron: new_iron,
        wood_remainder,
        clay_remainder,
        iron_remainder,
        wood_per_hour,
        clay_per_hour,
        iron_per_hour,
        resources_updated_at: effective_at,
        production_valid_until: boundary,
    })
}
