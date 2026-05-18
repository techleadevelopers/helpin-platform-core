use std::env;

use anyhow::Context;

#[derive(Clone, Debug)]
pub struct Config {
    pub bind_addr: String,
    pub database_url: String,
    pub redis_url: String,
    pub nats_url: String,
    pub ai_worker_url: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            bind_addr: env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".to_string()),
            database_url: env::var("DATABASE_URL").context("DATABASE_URL is required")?,
            redis_url: env::var("REDIS_URL")
                .unwrap_or_else(|_| "redis://localhost:6379".to_string()),
            nats_url: env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string()),
            ai_worker_url: env::var("AI_WORKER_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8090".to_string()),
        })
    }
}
