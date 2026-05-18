use sqlx::{postgres::PgPoolOptions, PgPool};

use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub db: PgPool,
}

impl AppState {
    pub async fn new(config: Config) -> anyhow::Result<Self> {
        let db = PgPoolOptions::new()
            .max_connections(10)
            .connect_lazy(&config.database_url)?;

        Ok(Self { config, db })
    }
}
