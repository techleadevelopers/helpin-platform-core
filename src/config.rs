use std::env;

use anyhow::Context;

#[derive(Clone, Debug)]
pub struct Config {
    pub app_env: String,
    pub bind_addr: String,
    pub database_url: String,
    pub database_max_connections: u32,
    pub database_min_connections: u32,
    pub redis_url: String,
    pub nats_url: String,
    pub ai_worker_url: String,
    pub jwt_secret: String,
    pub cloudinary_cloud_name: String,
    pub cloudinary_api_key: Option<String>,
    pub cloudinary_api_secret: Option<String>,
    pub geocoding_api_provider: Option<String>,
    pub google_maps_api_key: Option<String>,
    pub api_public_url: String,
    pub app_public_url: String,
    pub smtp_host: Option<String>,
    pub smtp_port: u16,
    pub smtp_user: Option<String>,
    pub smtp_pass: Option<String>,
    pub smtp_secure: bool,
    pub smtp_from_email: String,
    pub smtp_from_name: String,
    pub access_token_ttl_minutes: i64,
    pub refresh_token_ttl_days: i64,
    pub cors_allowed_origins: Vec<String>,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let config = Self {
            app_env: env::var("APP_ENV")
                .or_else(|_| env::var("RUST_ENV"))
                .unwrap_or_else(|_| "development".to_string()),
            bind_addr: env::var("PORT")
                .map(|port| format!("0.0.0.0:{port}"))
                .or_else(|_| env::var("BIND_ADDR"))
                .unwrap_or_else(|_| "127.0.0.1:8080".to_string()),
            database_url: env::var("DATABASE_URL").context("DATABASE_URL is required")?,
            database_max_connections: env::var("DATABASE_MAX_CONNECTIONS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(20),
            database_min_connections: env::var("DATABASE_MIN_CONNECTIONS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(1),
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
            api_public_url: env::var("API_PUBLIC_URL")
                .or_else(|_| env::var("EXPO_PUBLIC_API_BASE_URL"))
                .unwrap_or_else(|_| "https://zoohelp-core-production.up.railway.app".to_string()),
            app_public_url: env::var("APP_PUBLIC_URL")
                .unwrap_or_else(|_| "https://zoohelp.app".to_string()),
            smtp_host: env::var("SMTP_HOST")
                .ok()
                .or_else(|| env::var("MTP_HOST").ok()),
            smtp_port: env::var("SMTP_PORT")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(587),
            smtp_user: env::var("SMTP_USER").ok(),
            smtp_pass: env::var("SMTP_PASS").ok(),
            smtp_secure: env::var("SMTP_SECURE")
                .ok()
                .map(|value| matches!(value.as_str(), "true" | "1" | "yes" | "on"))
                .unwrap_or(true),
            smtp_from_email: env::var("SMTP_FROM_EMAIL")
                .ok()
                .or_else(|| env::var("SMTP_USER").ok())
                .unwrap_or_else(|| "no-reply@zoohelp.app".to_string()),
            smtp_from_name: env::var("SMTP_FROM_NAME").unwrap_or_else(|_| "ZooHelp".to_string()),
            access_token_ttl_minutes: env::var("ACCESS_TOKEN_TTL_MINUTES")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(15),
            refresh_token_ttl_days: env::var("REFRESH_TOKEN_TTL_DAYS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(30),
            cors_allowed_origins: env::var("CORS_ALLOWED_ORIGINS")
                .ok()
                .map(|value| {
                    value
                        .split(',')
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
        };

        config.validate()?;
        Ok(config)
    }

    pub fn is_development(&self) -> bool {
        matches!(self.app_env.as_str(), "development" | "dev" | "test")
    }

    fn validate(&self) -> anyhow::Result<()> {
        if self.is_development() {
            return Ok(());
        }

        anyhow::ensure!(
            self.jwt_secret != "dev-only-change-me-before-production"
                && self.jwt_secret.len() >= 32,
            "JWT_SECRET must be strong outside development"
        );
        anyhow::ensure!(
            self.cloudinary_api_key.is_some() && self.cloudinary_api_secret.is_some(),
            "Cloudinary credentials are required outside development"
        );
        anyhow::ensure!(
            !self.cors_allowed_origins.is_empty(),
            "CORS_ALLOWED_ORIGINS is required outside development"
        );
        Ok(())
    }
}

fn cloud_name_from_url(value: &str) -> Option<String> {
    value
        .rsplit_once('@')
        .map(|(_, cloud_name)| cloud_name.trim().trim_matches('/').to_string())
        .filter(|cloud_name| !cloud_name.is_empty())
}
