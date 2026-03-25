mod api;
mod billing;
mod infra;
mod models;
mod repo;
mod robot;

use anyhow::Result;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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
    repo.migrate().await?;
    let robot = Robot::new().await?;
    let billing = Billing::new(repo.clone(), robot.clone());
    let infra = Infra::new(repo.clone());
    let api = Api::new(repo);

    tokio::spawn(async move {
        billing.start().await;
    });

    tokio::spawn(async move {
        infra.start().await;
    });

    api.serve().await
}
