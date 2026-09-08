use crate::config::Config;
use openidconnect::core::CoreClient;
use sqlx::PgPool;
use std::sync::Arc;

pub struct AppState {
    pub config: Config,
    pub db: PgPool,
    pub google: CoreClient,
}

pub type SharedState = Arc<AppState>;
