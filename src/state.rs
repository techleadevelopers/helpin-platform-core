use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};

use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgPoolOptions, PgPool};
use tokio::sync::broadcast;

use crate::{config::Config, services::notifications::NotificationEngine};

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub db: PgPool,
    pub chat_tx: broadcast::Sender<ChatEvent>,
    pub notifications: NotificationEngine,
    pub rate_limiter: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
}

impl AppState {
    pub async fn new(config: Config) -> anyhow::Result<Self> {
        let db = PgPoolOptions::new()
            .max_connections(10)
            .connect_lazy(&config.database_url)?;
        let (chat_tx, _) = broadcast::channel(1024);

        Ok(Self {
            config,
            db,
            chat_tx,
            notifications: NotificationEngine::default(),
            rate_limiter: Arc::new(Mutex::new(HashMap::new())),
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatEvent {
    pub room_id: String,
    pub message_id: String,
    pub sender_id: String,
    pub body: String,
    pub created_at: String,
}
