//! The `caravel` binary: one process serving the relays, the API and the web
//! apps, and running the platform's background work.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use caravel_app::dispatch::Hosts;
use caravel_app::web::{PageConfig, Web};
use caravel_app::{config, dispatch, workers};
use caravel_core::Core;
use caravel_relay::Relays;
use caravel_util::blobs::Blobs;
use caravel_util::db::Db;
use caravel_util::livekit::Livekit;
use caravel_util::outbound::Outbound;
use caravel_util::{robot::Robot, stripe::Stripe, wallet::Wallet};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const USAGE: &str = "usage: caravel [serve]";

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] | ["serve"] => serve().await,
        _ => bail!(USAGE),
    }
}

async fn serve() -> Result<()> {
    let settings = config::load()?;

    let db = Db::connect(&settings.database_url, 20)
        .await
        .context("failed to connect to the database")?;
    db.migrate()
        .await
        .context("failed to migrate the database")?;

    let robot = Robot::new(
        &settings.config.platform_key.secret_hex(),
        settings.robot_profile,
    )?;
    let core = Core {
        db,
        config: Arc::new(settings.config),
        clock: Default::default(),
        bus: Default::default(),
        stripe: Stripe::new(&settings.stripe_secret_key),
        robot,
        wallet: Wallet::from_url(&settings.robot_wallet)
            .context("ROBOT_WALLET is not an NWC url")?,
        blobs: match &settings.media {
            config::Media::Local(dir) => Blobs::local(dir).context("MEDIA_DIR is not usable")?,
            config::Media::S3(s3) => Blobs::s3(s3).context("the media bucket is misconfigured")?,
        },
        livekit: settings
            .livekit
            .as_ref()
            .map(|livekit| Livekit::new(&livekit.url, &livekit.api_key, &livekit.api_secret))
            .transpose()?,
        outbound: Outbound::new(Duration::from_secs(10))?,
    };
    let background = core.with_db(Db::connect(&settings.database_url, 5).await?);

    let shutdown = CancellationToken::new();
    let tracker = TaskTracker::new();
    workers::spawn(&tracker, &shutdown, &background);

    let relays = Relays::load(core.clone(), settings.relays, shutdown.child_token()).await?;
    tracker.spawn({
        let (relays, shutdown) = (relays.clone(), shutdown.clone());
        async move { relays.watch(shutdown).await }
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
    let hosts = Hosts::from_urls(&core.config.api_url, core.config.platform_url.as_deref())?;
    let web = Web::new(PageConfig::new(&core.config, &settings.web));
    let app = dispatch::router(caravel_api::router(core).layer(cors), relays, web, hosts);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", settings.port)).await?;
    tracing::info!(port = settings.port, "serving");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(signal(shutdown.clone()))
    .await?;

    shutdown.cancel();
    tracker.close();
    if tokio::time::timeout(Duration::from_secs(10), tracker.wait())
        .await
        .is_err()
    {
        tracing::warn!("workers did not stop within ten seconds");
    }

    Ok(())
}

/// Resolves on SIGINT or SIGTERM, or when something else asks for shutdown.
async fn signal(shutdown: CancellationToken) {
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut terminate) => {
                terminate.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate => {}
        _ = shutdown.cancelled() => {}
    }

    shutdown.cancel();
}
