mod api;
mod billing;
mod bitcoin;
mod command;
mod env;
mod infra;
mod models;
mod pool;
mod query;
mod robot;
mod stripe;
mod wallet;

use anyhow::Result;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::api::Api;
use crate::billing::Billing;
use crate::command::Command;
use crate::env::Env;
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

    let env = Env::load();

    let pool = pool::create_pool(&env.database_url).await?;
    let robot = Robot::new(&env).await?;
    let query = Query::new(pool.clone(), &env);
    let command = Command::new(pool);
    let billing = Billing::new(query.clone(), command.clone(), robot.clone(), &env);
    let infra = Infra::new(query.clone(), command.clone(), &env);
    let api = Api::new(query, command, billing.clone(), infra.clone(), &env);

    let cors = if env.server_allow_origins.is_empty() {
        CorsLayer::permissive()
    } else {
        let parsed = env
            .server_allow_origins
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

    let listener =
        tokio::net::TcpListener::bind(format!("{}:{}", env.server_host, env.server_port)).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
