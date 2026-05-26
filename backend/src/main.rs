mod api;
mod billing;
mod bitcoin;
mod command;
mod env;
mod infra;
mod models;
mod db;
mod query;
mod robot;
mod routes;
mod stripe;
mod wallet;
mod web;

use anyhow::Result;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use tower_http::cors::{AllowOrigin, CorsLayer, Any};

use crate::api::Api;
use crate::billing::Billing;
use crate::infra::Infra;
use crate::robot::Robot;
use crate::stripe::Stripe;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .init();

    env::init();

    db::init().await?;

    let robot = Robot::new().await?;
    let stripe = Stripe::new();
    let infra = Infra::new();
    let billing = Billing::new(robot.clone());
    let api = Api::new(billing.clone(), stripe, robot, infra.clone());

    let parsed = env::get()
        .server_allow_origins
        .iter()
        .filter_map(|o| o.parse::<axum::http::HeaderValue>().ok())
        .collect::<Vec<_>>();
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(parsed))
        .allow_methods(Any)
        .allow_headers(Any);

    let app = api.router().layer(cors);

    tokio::spawn(async move {
        infra.start().await;
    });

    tokio::spawn(async move {
        billing.start().await;
    });

    let listener =
        tokio::net::TcpListener::bind(format!(
        "{}:{}",
        env::get().server_host,
        env::get().server_port
    ))
    .await?;
    axum::serve(listener, app).await?;
    Ok(())
}
