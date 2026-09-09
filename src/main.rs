mod auth;
mod config;
mod error;
mod jobs;
mod map;
mod middleware;
mod security;
mod state;
mod web;

use axum::{
    Json, Router,
    extract::State,
    middleware::from_fn,
    routing::{get, patch, post},
};

use config::Config;
use error::AppError;

use openidconnect::{
    ClientId, ClientSecret, IssuerUrl, RedirectUrl,
    core::{CoreClient, CoreProviderMetadata},
    reqwest::async_http_client,
};

use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use state::{AppState, SharedState};

use std::{sync::Arc, time::Duration};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = Config::from_env()?;

    let db = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&config.database_url)
        .await?;

    // Migration dosyaları binary içine gömülür.
    // Ayrı sqlx CLI kurmak zorunlu değildir.
    sqlx::migrate!("./migrations").run(&db).await?;

    tracing::info!("Database migrations applied");

    let google_metadata = CoreProviderMetadata::discover_async(
        IssuerUrl::new("https://accounts.google.com".to_owned())?,
        async_http_client,
    )
    .await?;

    let google = CoreClient::from_provider_metadata(
        google_metadata,
        ClientId::new(config.google_client_id.clone()),
        Some(ClientSecret::new(config.google_client_secret.clone())),
    )
    .set_redirect_uri(RedirectUrl::new(config.google_redirect_url())?);

    let bind_address = format!("{}:{}", config.host, config.port);
    let state: SharedState = Arc::new(AppState { config, db, google });

    let app = Router::new()
        .route("/map", get(map::page))
        .route("/assets/map.css", get(map::stylesheet))
        .route("/assets/map.js", get(map::javascript))
        .route("/api/map/bootstrap", get(map::bootstrap))
        .route("/api/map/join", post(map::join))
        .route("/api/map/area", get(map::area))
        .route("/", get(web::index))
        .route("/game", get(web::game))
        .route("/assets/game.css", get(web::stylesheet))
        .route("/assets/game.js", get(web::javascript))
        .route(
            "/api/game",
            get(web::api::bootstrap).layer(from_fn(
                |request: axum::extract::Request, next: axum::middleware::Next| async move {
                    let mut response = next.run(request).await;

                    response.headers_mut().insert(
                        axum::http::header::CACHE_CONTROL,
                        "no-store".parse().unwrap(),
                    );

                    response
                },
            )),
        )
        .route("/api/villages", post(web::api::create_village))
        .route("/api/village/name", patch(web::api::rename_village))
        .route(
            "/api/buildings/{kind}/upgrade",
            post(web::api::start_upgrade),
        )
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .route("/auth/google", get(auth::handlers::google_login))
        .route(
            "/auth/google/callback",
            get(auth::handlers::google_callback),
        )
        .route("/auth/me", get(auth::handlers::me))
        .route("/auth/logout", post(auth::handlers::logout))
        .route(
            "/internal/jobs/{job_id}/execute",
            post(jobs::handlers::execute),
        )
        .layer(from_fn(middleware::request_logger))
        .with_state(state.clone());

    let cleanup_db = state.db.clone();

    let cleanup_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(15 * 60));

        loop {
            interval.tick().await;

            if let Err(error) = auth::repository::cleanup_expired(&cleanup_db).await {
                tracing::error!(
                    %error,
                    "Expired authentication records cleanup failed"
                );
            }
        }
    });

    let listener = TcpListener::bind(&bind_address).await?;

    tracing::info!(
        address = %bind_address,
        "Rust API started"
    );

    let result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await;

    cleanup_task.abort();
    state.db.close().await;

    result?;

    Ok(())
}

async fn live() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn ready(State(state): State<SharedState>) -> Result<Json<Value>, AppError> {
    sqlx::query("SELECT 1").execute(&state.db).await?;

    Ok(Json(json!({
        "status": "ok",
        "postgres": "connected"
    })))
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install terminate handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Shutdown requested");
}
