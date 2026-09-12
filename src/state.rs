use crate::config::Config;
use crate::realtime::MatchHub;
use crate::store::Redis;
use openidconnect::core::CoreClient;
use std::sync::Arc;

pub struct AppState {
    pub config: Config,
    pub redis: Redis,
    pub google: CoreClient,
    pub hub: MatchHub,
}

pub type SharedState = Arc<AppState>;
