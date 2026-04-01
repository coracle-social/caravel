mod api;
mod billing;
mod infra;
mod models;
mod repo;
mod robot;

use anyhow::Result;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::api::Api;
use crate::billing::Billing;
use crate::infra::Infra;
use crate::repo::Repo;
use crate::robot::Robot;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .init();

    let repo = Repo::new().await?;
    let robot = Robot::new().await?;
    let billing = Billing::new(repo.clone(), robot.clone());
    let infra = Infra::new(repo.clone());
    let api = Api::new(repo, billing.clone());

    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3000);
    let origins: Vec<String> = std::env::var("ALLOW_ORIGINS")
        .unwrap_or_default()
        .split(',')
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect();

    let cors = if origins.is_empty() {
        CorsLayer::permissive()
    } else {
        let parsed = origins
            .iter()
            .filter_map(|o| o.parse::<axum::http::HeaderValue>().ok())
            .collect::<Vec<_>>();
        CorsLayer::new().allow_origin(AllowOrigin::list(parsed))
    };

    let app = api.router().layer(cors);

    tokio::spawn(async move {
        infra.start().await;
    });

    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}")).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
