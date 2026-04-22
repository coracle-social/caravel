mod api;
mod billing;
mod command;
mod infra;
mod models;
mod pool;
mod query;
mod robot;

use anyhow::Result;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::api::Api;
use crate::billing::Billing;
use crate::command::Command;
use crate::infra::Infra;
use crate::query::Query;
use crate::robot::Robot;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .init();

    let pool = pool::create_pool().await?;
    let robot = Robot::new().await?;
    let query = Query::new(pool.clone());
    let command = Command::new(pool);
    let billing = Billing::new(query.clone(), command.clone(), robot.clone());
    let infra = Infra::new(query.clone(), command.clone())?;
    let api = Api::new(query, command, billing.clone(), infra.clone());

    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2892);
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

    tokio::spawn(async move {
        billing.start().await;
    });

    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}")).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
