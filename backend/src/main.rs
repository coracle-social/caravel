mod api;
mod auth;
mod config;
mod db;
mod models;
mod provisioning;
mod repo;

use std::net::SocketAddr;

use anyhow::Result;
use axum::{routing::get, Router};
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::Config;
use crate::db::init_pool;
use crate::provisioning::Provisioner;
use crate::repo::Repo;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config::from_env();
    ensure_sqlite_dir(&config.database_url)?;

    let pool = init_pool(&config.database_url).await?;
    let repo = Repo::new(pool);
    let provisioner = Provisioner::new(
        config.zooid_api_url.clone(),
        config.relay_domain.clone(),
        config.platform_secret.clone(),
    )?;
    let state = api::AppState {
        repo,
        admin_pubkeys: std::sync::Arc::new(config.admin_pubkeys.clone()),
        provisioner,
    };

    let app = Router::new()
        .merge(api::router(state))
        .route("/healthz", get(healthz))
        .layer(CorsLayer::permissive());

    let addr: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("listening on {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}

fn ensure_sqlite_dir(database_url: &str) -> Result<()> {
    if let Some(path) = database_url.strip_prefix("sqlite://") {
        if let Some(dir) = std::path::Path::new(path).parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)?;
            }
        }
    }
    Ok(())
}

async fn healthz() -> &'static str {
    "ok"
}
