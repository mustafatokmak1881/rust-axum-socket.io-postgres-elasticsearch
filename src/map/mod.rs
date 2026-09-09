use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, header},
    response::{Html, IntoResponse, Redirect, Response},
};

use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    auth::repository::{self, CurrentUser},
    error::AppError,
    security::hash_token,
    state::SharedState,
};

fn world_id() -> Uuid {
    Uuid::from_u128(1)
}

#[derive(Serialize, FromRow)]
pub struct World {
    pub id: Uuid,
    pub name: String,
    pub width: i32,
    pub height: i32,
    pub map_seed: i32,
}

#[derive(Serialize, FromRow)]
pub struct Village {
    pub id: Uuid,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub points: i32,
}

#[derive(Serialize)]
pub struct Bootstrap {
    pub world: World,
    pub village: Option<Village>,
}

#[derive(Deserialize)]
pub struct MapBounds {
    pub min_x: i32,
    pub max_x: i32,
    pub min_y: i32,
    pub max_y: i32,
}

#[derive(Serialize, FromRow)]
pub struct MapVillage {
    pub id: Uuid,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub points: i32,
    pub affiliation: String,
}

#[derive(Serialize)]
pub struct MapResponse {
    pub villages: Vec<MapVillage>,
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

pub async fn page(
    State(state): State<SharedState>,
    jar: CookieJar,
) -> Result<Response, AppError> {
    match current_user(&state, &jar).await {
        Ok(_) => {}
        Err(AppError::Unauthorized) => {
            return Ok(Redirect::to("/auth/google").into_response());
        }
        Err(error) => return Err(error),
    }

    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Html(include_str!("map.html")),
    )
        .into_response())
}

pub async fn stylesheet() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("map.css"),
    )
}

pub async fn javascript() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "text/javascript; charset=utf-8",
        )],
        include_str!("map.js"),
    )
}

pub async fn bootstrap(
    State(state): State<SharedState>,
    jar: CookieJar,
) -> Result<Json<Bootstrap>, AppError> {
    let user = current_user(&state, &jar).await?;

    let world = sqlx::query_as::<_, World>(
        r#"
        SELECT id, name, width, height, map_seed
        FROM worlds
        WHERE id = $1
        "#,
    )
    .bind(world_id())
    .fetch_one(&state.db)
    .await?;

    let village = sqlx::query_as::<_, Village>(
        r#"
        SELECT id, name, x, y, points
        FROM villages
        WHERE world_id = $1 AND owner_id = $2
        ORDER BY created_at, id
        LIMIT 1
        "#,
    )
    .bind(world_id())
    .bind(user.id)
    .fetch_optional(&state.db)
    .await?;

    Ok(Json(Bootstrap { world, village }))
}

// İlk köy oluşturma durum değiştirir: GET yerine POST.
// Cookie oturumu + Origin kontrolü uygulanır.
pub async fn join(
    State(state): State<SharedState>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<Village>, AppError> {
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());

    if origin != Some(state.config.app_origin.as_str()) {
        return Err(AppError::Forbidden);
    }

    let user = current_user(&state, &jar).await?;
    let mut transaction = state.db.begin().await?;

    // İlk sürümde köy yerleştirmelerini sıraya alır.
    // Aynı anda iki sekme açılması veya farklı Rust instance'ları
    // aynı kullanıcı için iki başlangıç köyü oluşturmaz.
    sqlx::query("SELECT pg_advisory_xact_lock(731001::bigint)")
        .execute(&mut *transaction)
        .await?;

    let existing = sqlx::query_as::<_, Village>(
        r#"
        SELECT id, name, x, y, points
        FROM villages
        WHERE world_id = $1 AND owner_id = $2
        ORDER BY created_at, id
        LIMIT 1
        "#,
    )
    .bind(world_id())
    .bind(user.id)
    .fetch_optional(&mut *transaction)
    .await?;

    if let Some(village) = existing {
        transaction.commit().await?;
        return Ok(Json(village));
    }

    // Başlangıç oyuncuları merkez çevresinde yerleşir.
    // Bu geliştirme sürümünde başlangıç alanı 100 x 100 kare.
    let position = sqlx::query_as::<_, (i32, i32)>(
        r#"
        SELECT gx.x, gy.y
        FROM generate_series(450, 549) AS gx(x)
        CROSS JOIN generate_series(450, 549) AS gy(y)
        WHERE NOT EXISTS (
            SELECT 1
            FROM villages v
            WHERE v.world_id = $1
              AND v.x = gx.x
              AND v.y = gy.y
        )
        ORDER BY md5(
            gx.x::text || ':' ||
            gy.y::text || ':' ||
            $2::text
        )
        LIMIT 1
        "#,
    )
    .bind(world_id())
    .bind(user.id.to_string())
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::BadRequest(
        "Starting area is full",
    ))?;

    let village = sqlx::query_as::<_, Village>(
        r#"
        INSERT INTO villages (
            id,
            world_id,
            owner_id,
            name,
            x,
            y,
            points
        )
        VALUES ($1, $2, $3, $4, $5, $6, 0)
        RETURNING id, name, x, y, points
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(world_id())
    .bind(user.id)
    .bind("Yeni Oba")
    .bind(position.0)
    .bind(position.1)
    .fetch_one(&mut *transaction)
    .await?;

    transaction.commit().await?;

    Ok(Json(village))
}

pub async fn area(
    State(state): State<SharedState>,
    jar: CookieJar,
    Query(bounds): Query<MapBounds>,
) -> Result<Json<MapResponse>, AppError> {
    let user = current_user(&state, &jar).await?;

    // Önce sınırlar, ardından genişlik kontrol edilir.
    // Böylece kontrolsüz büyük sorgular ve aritmetik taşma önlenir.
    if bounds.min_x < 0
        || bounds.min_y < 0
        || bounds.max_x > 999
        || bounds.max_y > 999
        || bounds.max_x < bounds.min_x
        || bounds.max_y < bounds.min_y
    {
        return Err(AppError::BadRequest("Invalid map bounds"));
    }

    if bounds.max_x - bounds.min_x + 1 > 100
        || bounds.max_y - bounds.min_y + 1 > 100
    {
        return Err(AppError::BadRequest(
            "Map area cannot exceed 100 x 100 tiles",
        ));
    }

    let villages = sqlx::query_as::<_, MapVillage>(
        r#"
        SELECT
            id,
            name,
            x,
            y,
            points,
            CASE
                WHEN owner_id = $2 THEN 'own'
                WHEN owner_id IS NULL THEN 'barbarian'
                ELSE 'player'
            END AS affiliation
        FROM villages
        WHERE world_id = $1
          AND x BETWEEN $3 AND $4
          AND y BETWEEN $5 AND $6
        ORDER BY y, x
        "#,
    )
    .bind(world_id())
    .bind(user.id)
    .bind(bounds.min_x)
    .bind(bounds.max_x)
    .bind(bounds.min_y)
    .bind(bounds.max_y)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(MapResponse { villages }))
}