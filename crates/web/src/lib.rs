//! Web server

mod config;

use std::sync::Arc;

use axum::Router;
use axum_extra::routing::SpaRouter;

use config::Config;
use tracing::info;

pub struct Web {
    config: Config,
}

impl Web {
    /// Initializes the `Web` component from environment variables.
    pub fn new_from_env() -> Self {
        Self::new(Config::env())
    }

    pub fn new(config: Config) -> Self {
        info!(
            "Initializing web app server. Asset directory: {}",
            config.asset_dir
        );

        Self { config }
    }

    pub fn routes(self: Arc<Self>) -> Router {
        SpaRouter::new("/assets", &self.config.asset_dir).into()
    }
}
