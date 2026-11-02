//! The `caravel` binary: one process serving the relays, the API and the web
//! apps, and running the platform's background work.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use caravel_app::web::{PageConfig, Web};
use caravel_app::{config, dispatch};
use caravel_core::Core;
use caravel_relay::Relays;
use caravel_util::blobs::Blobs;
use caravel_util::bus::Bus;
use caravel_util::clock::Clock;
use caravel_util::db::Db;
use caravel_util::livekit::Livekit;
use caravel_util::outbound::Outbound;
use caravel_util::{robot::Robot, stripe::Stripe, wallet::Wallet};
use caravel_worker as workers;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const USAGE: &str = "usage: caravel serve";

/// What the services share outside the core.
struct AppContext {
    settings: config::Settings,
    shutdown: CancellationToken,
    tracker: TaskTracker,
}

impl AppContext {
    /// Serves until a signal or another caller cancels, then stops every service
    /// and waits up to ten seconds for them.
    async fn run(&self) {
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
            _ = self.shutdown.cancelled() => {}
        }

        self.shutdown.cancel();
        self.tracker.close();

        if tokio::time::timeout(Duration::from_secs(10), self.tracker.wait())
            .await
            .is_err()
        {
            tracing::warn!("services did not stop within ten seconds");
        }
    }
}

/// Sends logs to stdout at the level `RUST_LOG` asks for.
fn init_logger() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .init();
}

/// Connects a pool of `connections` connections, bringing the schema up to date.
async fn build_db(settings: &config::Settings, connections: u32) -> Result<Db> {
    let db = Db::connect(&settings.database_url, connections)
        .await
        .context("failed to connect to the database")?;
    db.migrate()
        .await
        .context("failed to migrate the database")?;

    Ok(db)
}

fn build_robot(settings: &config::Settings) -> Result<Robot> {
    Robot::new(
        &settings.config.platform_key.secret_hex(),
        settings.robot_profile.clone(),
    )
}

fn build_blobs(settings: &config::Settings) -> Result<Blobs> {
    Blobs::s3(&settings.media).context("the media bucket is misconfigured")
}

fn build_stripe(settings: &config::Settings) -> Stripe {
    Stripe::new(&settings.stripe_secret_key)
}

fn build_wallet(settings: &config::Settings) -> Result<Wallet> {
    Wallet::from_url(&settings.robot_wallet).context("ROBOT_WALLET is not an NWC url")
}

fn build_livekit(settings: &config::Settings) -> Result<Option<Livekit>> {
    settings
        .livekit
        .as_ref()
        .map(|livekit| Livekit::new(&livekit.url, &livekit.api_key, &livekit.api_secret))
        .transpose()
}

/// The one core the services share, on the pool the schema is migrated through.
/// Each service takes it onto a pool of its own before it starts.
async fn build_core(settings: &config::Settings) -> Result<Core> {
    Ok(Core {
        db: build_db(settings, 1).await?,
        config: Arc::new(settings.config.clone()),
        clock: Clock::default(),
        bus: Bus::default(),
        stripe: build_stripe(settings),
        robot: build_robot(settings)?,
        wallet: build_wallet(settings)?,
        blobs: build_blobs(settings)?,
        livekit: build_livekit(settings)?,
        outbound: Outbound::new(Duration::from_secs(10))?,
    })
}

/// Reads the settings, and sets up the shutdown the services stop through.
fn build_ctx() -> Result<AppContext> {
    Ok(AppContext {
        settings: config::load()?,
        shutdown: CancellationToken::new(),
        tracker: TaskTracker::new(),
    })
}

/// Sets up cors, and serves the API, the web apps and the relays until shutdown.
async fn start_router(ctx: &AppContext, core: Core) -> Result<()> {
    let relays = start_relays(ctx, core.clone()).await?;

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
    let platform = core.config.platform_host();
    let web = Web::new(PageConfig::new(&core.config, &ctx.settings.web));
    let app = dispatch::router(caravel_api::router(core).layer(cors), relays, web, platform);

    let port = ctx.settings.port;
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!(port, "serving");

    let shutdown = ctx.shutdown.clone();
    ctx.tracker.spawn(async move {
        let served = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(async move { shutdown.cancelled().await })
        .await;
        if let Err(error) = served {
            tracing::error!(error = ?error, "the server stopped");
        }
    });

    Ok(())
}

/// Runs the platform's background work, until shutdown.
fn start_worker(ctx: &AppContext, core: Core) {
    workers::spawn(&ctx.tracker, &ctx.shutdown, &core);
}

/// Loads the live relays and watches for changes to them, until shutdown.
async fn start_relays(ctx: &AppContext, core: Core) -> Result<Relays> {
    let relays = Relays::load(
        core,
        ctx.settings.relays.clone(),
        ctx.shutdown.child_token(),
    )
    .await?;

    ctx.tracker.spawn({
        let (relays, shutdown) = (relays.clone(), ctx.shutdown.clone());
        async move { relays.watch(shutdown).await }
    });

    Ok(relays)
}

#[tokio::main]
async fn main() -> Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("serve") => {}
        _ => bail!(USAGE),
    }

    let ctx = build_ctx()?;
    let core = build_core(&ctx.settings).await?;

    init_logger();

    start_router(&ctx, core.with_database(build_db(&ctx.settings, 20).await?)).await?;
    start_worker(&ctx, core.with_database(build_db(&ctx.settings, 5).await?));

    ctx.run().await;

    Ok(())
}
