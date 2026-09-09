mod api;
mod billing;
mod bitcoin;
mod command;
mod db;
mod domains;
mod env;
mod infra;
mod models;
mod query;
mod robot;
mod routes;
mod stripe;
mod wallet;
mod web;

use anyhow::Result;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use tower_http::cors::{AllowOrigin, Any, CorsLayer};

use crate::api::Api;
use crate::billing::Billing;
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
    let billing = Billing::new(robot.clone());
    let api = Api::new(billing.clone(), stripe, robot);

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _| {
            origin.to_str().is_ok_and(|o| env::get().allows_origin(o))
        }))
        .allow_methods(Any)
        .allow_headers(Any);

    let app = api.router().layer(cors);

    tokio::spawn(async {
        infra::start().await;
    });
    tokio::spawn(async {
        infra::start_domain_verification().await;
    });
    tokio::spawn(async {
        infra::start_domain_reverification().await;
    });

    tokio::spawn(async move {
        billing.start().await;
    });

    let url = format!("0.0.0.0:{}", env::get().server_port);
    let listener = tokio::net::TcpListener::bind(url).await?;

    axum::serve(listener, app).await?;

    Ok(())
}
