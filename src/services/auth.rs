use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{config::Config, domain::AccountType};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessClaims {
    pub sub: String,
    pub email: String,
    pub account_type: AccountType,
    pub exp: usize,
    pub iat: usize,
}

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|error| anyhow::anyhow!("password hashing failed: {error}"))?
        .to_string())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    let Ok(parsed_hash) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}

pub fn issue_access_token(
    config: &Config,
    user_id: &str,
    email: &str,
    account_type: AccountType,
) -> anyhow::Result<String> {
    let now = Utc::now();
    let exp = now + Duration::minutes(config.access_token_ttl_minutes);
    let claims = AccessClaims {
        sub: user_id.to_string(),
        email: email.to_string(),
        account_type,
        exp: exp.timestamp() as usize,
        iat: now.timestamp() as usize,
    };
    Ok(encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
    )?)
}

#[allow(dead_code)]
pub fn verify_access_token(config: &Config, token: &str) -> anyhow::Result<AccessClaims> {
    Ok(decode::<AccessClaims>(
        token,
        &DecodingKey::from_secret(config.jwt_secret.as_bytes()),
        &Validation::default(),
    )?
    .claims)
}

pub fn new_refresh_token() -> String {
    Uuid::now_v7().to_string()
}

pub fn account_type_as_str(account_type: &AccountType) -> &'static str {
    match account_type {
        AccountType::Person => "person",
        AccountType::Ong => "ong",
        AccountType::Vet => "vet",
        AccountType::Admin => "admin",
    }
}

pub fn account_type_from_str(value: &str) -> AccountType {
    match value {
        "ong" => AccountType::Ong,
        "vet" => AccountType::Vet,
        "admin" => AccountType::Admin,
        _ => AccountType::Person,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_hash_verifies_and_rejects_wrong_password() {
        let hash = hash_password("senha-segura").expect("hash");

        assert!(verify_password("senha-segura", &hash));
        assert!(!verify_password("senha-errada", &hash));
    }

    #[test]
    fn access_token_round_trips_claims() {
        let config = Config {
            app_env: "test".into(),
            bind_addr: "127.0.0.1:0".into(),
            database_url: "postgres://zoohelp:zoohelp@localhost:5432/zoohelp".into(),
            database_max_connections: 5,
            database_min_connections: 0,
            redis_url: "redis://localhost:6379".into(),
            nats_url: "nats://localhost:4222".into(),
            ai_worker_url: "http://127.0.0.1:8090".into(),
            jwt_secret: "test-secret".into(),
            cloudinary_cloud_name: "limpeja".into(),
            cloudinary_api_key: Some("test-api-key".into()),
            cloudinary_api_secret: Some("test-api-secret".into()),
            geocoding_api_provider: Some("google".into()),
            google_maps_api_key: Some("test-google-key".into()),
            api_public_url: "https://api.zoohelp.test".into(),
            app_public_url: "https://zoohelp.test".into(),
            smtp_host: None,
            smtp_port: 587,
            smtp_user: None,
            smtp_pass: None,
            smtp_secure: true,
            smtp_from_email: "no-reply@zoohelp.test".into(),
            smtp_from_name: "ZooHelp".into(),
            access_token_ttl_minutes: 15,
            refresh_token_ttl_days: 30,
            cors_allowed_origins: Vec::new(),
            postgis_enabled: false,
            payments_enabled: false,
            payment_provider: "test".into(),
            payment_webhook_secret: Some("test-webhook-secret-123456".into()),
            sentry_dsn: None,
            otel_exporter_otlp_endpoint: None,
            push_worker_enabled: false,
            push_provider: "expo".into(),
        };

        let token =
            issue_access_token(&config, "u1", "user@zoohelp.com", AccountType::Ong).expect("token");
        let claims = verify_access_token(&config, &token).expect("claims");

        assert_eq!(claims.sub, "u1");
        assert_eq!(claims.account_type, AccountType::Ong);
    }
}
