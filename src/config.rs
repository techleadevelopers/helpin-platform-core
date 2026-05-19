use std::env;

use anyhow::Context;

#[derive(Clone, Debug)]
pub struct Config {
    pub bind_addr: String,
    pub database_url: String,
    pub redis_url: String,
    pub nats_url: String,
    pub ai_worker_url: String,
    pub jwt_secret: String,
    pub cloudinary_cloud_name: String,
    pub cloudinary_api_key: Option<String>,
    pub cloudinary_api_secret: Option<String>,
    pub geocoding_api_provider: Option<String>,
    pub google_maps_api_key: Option<String>,
    pub access_token_ttl_minutes: i64,
    pub refresh_token_ttl_days: i64,
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
            jwt_secret: env::var("JWT_SECRET")
                .unwrap_or_else(|_| "dev-only-change-me-before-production".to_string()),
            cloudinary_cloud_name: env::var("CLOUDINARY_CLOUD_NAME")
                .ok()
                .or_else(|| cloud_name_from_url(&env::var("CLOUDINARY_URL").ok()?))
                .unwrap_or_else(|| "zoohelp-dev".to_string()),
            cloudinary_api_key: env::var("CLOUDINARY_API_KEY").ok(),
            cloudinary_api_secret: env::var("CLOUDINARY_API_SECRET").ok(),
            geocoding_api_provider: env::var("GEOCODING_API_PROVIDER").ok(),
            google_maps_api_key: env::var("GOOGLE_MAPS_API_KEY").ok(),
            access_token_ttl_minutes: env::var("ACCESS_TOKEN_TTL_MINUTES")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(15),
            refresh_token_ttl_days: env::var("REFRESH_TOKEN_TTL_DAYS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(30),
        })
    }
}

fn cloud_name_from_url(value: &str) -> Option<String> {
    value
        .rsplit_once('@')
        .map(|(_, cloud_name)| cloud_name.trim().trim_matches('/').to_string())
        .filter(|cloud_name| !cloud_name.is_empty())
}
