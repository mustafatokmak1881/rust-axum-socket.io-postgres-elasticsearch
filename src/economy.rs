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

/// Yeni köyde seviye 1 başlayan binalar.
pub const STARTING_BUILDINGS: &[(&str, i32)] = &[
    ("headquarters", 1),
    ("timber", 1),
    ("clay", 1),
    ("iron", 1),
    ("farm", 1),
    ("warehouse", 1),
    ("rally_point", 1),
    ("hiding_place", 1),
];

// 1× dünya için referans değerler.
// Index 0 = bina yok / seviye 0.
const TIMBER_PRODUCTION: [i64; 31] = [
    0, 30, 35, 41, 47, 55, 64, 74, 86, 100, 117, 136, 158, 184, 214, 249, 289,
    337, 391, 455, 530, 616, 717, 833, 969, 1127, 1311, 1525, 1774, 2063, 2400,
];

pub fn timber_production(level: i32) -> Result<i64, AppError> {
    if !(0..=30).contains(&level) {
        return Err(AppError::Internal(anyhow::anyhow!(
            "Invalid timber level: {level}"
        )));
    }

    Ok(TIMBER_PRODUCTION[level as usize])
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

// Bunlar mevcut geliştirme maliyetleri.
// Henüz Tribal Wars maliyet/süre tablolarına dönüştürülmedi.
pub fn upgrade_cost(target_level: i32) -> i64 {
    i64::from(target_level) * 100
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
    wood_remainder: i64,
    resources_updated_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct EconomySnapshot {
    pub wood: i64,
    pub wood_remainder: i64,
    pub wood_per_hour: i64,
    pub resources_updated_at: DateTime<Utc>,

    // Bu andan sonra bina seviyesi değişebilir.
    // İstemci bu sınırın ötesine eski hızla üretim tahmini yapmamalı.
    pub production_valid_until: Option<DateTime<Utc>>,
}

/// Mevcut transaction içinde çağrılır.
///
/// Köy satırını kilitler; üretilen tam odunu ve kesirli payı kaydeder.
/// Vadesi gelmiş ama worker tarafından tamamlanmamış inşaat varsa
/// hesaplama onun run_at zamanında durur.
///
/// Burada scheduled_jobs satırı kilitlenmez/güncellenmez.
/// Worker'ın job -> village kilit sırasıyla ters kilit oluşmaz.
pub async fn settle(
    connection: &mut PgConnection,
    village_id: Uuid,
    requested_at: DateTime<Utc>,
) -> Result<EconomySnapshot, AppError> {
    let row = sqlx::query_as::<_, ResourceRow>(
        r#"
        SELECT wood, wood_remainder, resources_updated_at
        FROM villages
        WHERE id = $1
        FOR UPDATE
        "#,
    )
    .bind(village_id)
    .fetch_one(&mut *connection)
    .await?;

    let timber_level = sqlx::query_scalar::<_, i32>(
        r#"
        SELECT level
        FROM village_buildings
        WHERE village_id = $1 AND kind = 'timber'
        "#,
    )
    .bind(village_id)
    .fetch_optional(&mut *connection)
    .await?
    .unwrap_or(0);

    let wood_per_hour = timber_production(timber_level)?;

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

    // Migration sonrası geçmişte kalmış bir iş veya saat düzeltmesi
    // hesaplama zamanını geriye götürmemeli.
    if effective_at < row.resources_updated_at {
        effective_at = row.resources_updated_at;
    }

    let elapsed_micros = effective_at
        .signed_duration_since(row.resources_updated_at)
        .num_microseconds()
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("Resource time interval is too large"))
        })?;

    let accumulated =
        i128::from(row.wood_remainder) + i128::from(elapsed_micros) * i128::from(wood_per_hour);

    let produced = accumulated / MICROS_PER_HOUR;
    let remainder = (accumulated % MICROS_PER_HOUR) as i64;

    let new_wood = i64::try_from(i128::from(row.wood) + produced).map_err(|_| {
        AppError::Internal(anyhow::anyhow!("Wood balance overflow"))
    })?;

    sqlx::query(
        r#"
        UPDATE villages
        SET wood = $2,
            wood_remainder = $3,
            resources_updated_at = $4
        WHERE id = $1
        "#,
    )
    .bind(village_id)
    .bind(new_wood)
    .bind(remainder)
    .bind(effective_at)
    .execute(&mut *connection)
    .await?;

    Ok(EconomySnapshot {
        wood: new_wood,
        wood_remainder: remainder,
        wood_per_hour,
        resources_updated_at: effective_at,
        production_valid_until: boundary,
    })
}
