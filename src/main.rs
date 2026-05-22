use std::net::SocketAddr;

use anyhow::Context;
use axum::http::{HeaderValue, Method};
use tokio::net::TcpListener;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod domain;
mod error;
mod routes;
mod services;
mod state;

use config::Config;
use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    let config = Config::from_env()?;
    let state = AppState::new(config.clone()).await?;

    let app = routes::router(state)
        .layer(cors_layer(&config)?)
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = config.bind_addr.parse().context("invalid BIND_ADDR")?;
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(%addr, "zoohelp rust backend listening");

    axum::serve(listener, app).await?;
    Ok(())
}

fn cors_layer(config: &Config) -> anyhow::Result<CorsLayer> {
    let methods = [
        Method::GET,
        Method::POST,
        Method::PUT,
        Method::PATCH,
        Method::DELETE,
        Method::OPTIONS,
    ];

    if config.is_development() && config.cors_allowed_origins.is_empty() {
        return Ok(CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(methods)
            .allow_headers(Any));
    }

    let origins = config
        .cors_allowed_origins
        .iter()
        .map(|origin| origin.parse::<HeaderValue>())
        .collect::<Result<Vec<_>, _>>()
        .context("invalid CORS_ALLOWED_ORIGINS")?;

    Ok(CorsLayer::new()
        .allow_origin(origins)
        .allow_methods(methods)
        .allow_headers(Any))
}
