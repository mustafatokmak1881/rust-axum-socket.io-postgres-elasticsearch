mod auth;
mod config;
mod error;
mod middleware;
mod monetization;
mod play;
mod realtime;
mod security;
mod state;
mod store;

use axum::{
    Json, Router,
    extract::State,
    middleware::from_fn,
    routing::{get, post},
};
use config::Config;
use error::AppError;
use openidconnect::{
    ClientId, ClientSecret, IssuerUrl, RedirectUrl,
    core::{CoreClient, CoreProviderMetadata},
    reqwest::async_http_client,
};
use serde_json::{Value, json};
use state::{AppState, SharedState};
use std::sync::Arc;
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
    let redis = store::connect(&config.redis_url).await?;
    tracing::info!("Connected to Redis");

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

    let hub = realtime::MatchHub::new(redis.clone());

    let state: SharedState = Arc::new(AppState {
        config,
        redis: redis.clone(),
        google,
        hub,
    });

    let app = Router::new()
        .route("/", get(play::index))
        .merge(play::router())
        .merge(monetization::router())
        .route("/ws", get(realtime::ws::ws_upgrade))
        .route("/api/lobbies", get(list_lobbies))
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .route("/auth/google", get(auth::handlers::google_login))
        .route(
            "/auth/google/callback",
            get(auth::handlers::google_callback),
        )
        .route("/auth/me", get(auth::handlers::me))
        .route("/auth/logout", post(auth::handlers::logout))
        .layer(from_fn(middleware::request_logger))
        .with_state(state.clone());

    let bind_address = format!("{}:{}", state.config.host, state.config.port);
    let listener = TcpListener::bind(&bind_address).await?;
    tracing::info!(address = %bind_address, "Koalisyon realtime API started");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn live() -> Json<Value> {
    Json(json!({ "status": "ok", "mode": "realtime" }))
}

async fn ready(State(state): State<SharedState>) -> Result<Json<Value>, AppError> {
    let mut conn = state.redis.clone();
    let pong: String = redis::cmd("PING").query_async(&mut conn).await?;
    Ok(Json(json!({
        "status": "ok",
        "redis": pong,
    })))
}

async fn list_lobbies(State(state): State<SharedState>) -> Json<Value> {
    let matches = state.hub.list_open_matches().await;
    Json(json!({ "matches": matches, "lobbies": matches }))
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
