use axum::{
    Json,
    extract::{Query, State},
    http::header,
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
) -> Result<impl IntoResponse, AppError> {
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
        SELECT
            v.id,
            v.name,
            v.x,
            v.y,
            COALESCE(SUM(b.level), 0)::INTEGER AS points
        FROM villages v
        LEFT JOIN village_buildings b
            ON b.village_id = v.id
        WHERE v.world_id = $1
          AND v.owner_id = $2
        GROUP BY v.id, v.name, v.x, v.y
        "#,
    )
    .bind(world_id())
    .bind(user.id)
    .fetch_optional(&state.db)
    .await?;

    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(Bootstrap { world, village }),
    ))
}

pub async fn area(
    State(state): State<SharedState>,
    jar: CookieJar,
    Query(bounds): Query<MapBounds>,
) -> Result<impl IntoResponse, AppError> {
    let user = current_user(&state, &jar).await?;

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
            v.id,
            v.name,
            v.x,
            v.y,
            COALESCE(SUM(b.level), 0)::INTEGER AS points,
            CASE
                WHEN v.owner_id = $2 THEN 'own'
                ELSE 'player'
            END AS affiliation
        FROM villages v
        LEFT JOIN village_buildings b
            ON b.village_id = v.id
        WHERE v.world_id = $1
          AND v.x BETWEEN $3 AND $4
          AND v.y BETWEEN $5 AND $6
        GROUP BY v.id, v.name, v.x, v.y, v.owner_id
        ORDER BY v.y, v.x
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

    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(MapResponse { villages }),
    ))
}