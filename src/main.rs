use std::net::SocketAddr;

use anyhow::Context;
use tokio::net::TcpListener;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
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
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = config.bind_addr.parse().context("invalid BIND_ADDR")?;
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(%addr, "zoohelp rust backend listening");

    axum::serve(listener, app).await?;
    Ok(())
}
