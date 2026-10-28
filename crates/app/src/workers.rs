//! Background work: when each periodic job runs and what wakes the reactive ones.
//! The jobs themselves live in the core.

use std::future::Future;
use std::time::Duration;

use caravel_core::{Core, billing, domains, events, hosting, memberships, sessions};
use caravel_domain::Result;
use tokio::time::{Instant, interval_at};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

const MINUTE: Duration = Duration::from_secs(60);
const HOUR: Duration = Duration::from_secs(60 * 60);

/// Start every worker on `core`, each stopping when `shutdown` is cancelled.
pub fn spawn(tracker: &TaskTracker, shutdown: &CancellationToken, core: &Core) {
    every(
        tracker,
        shutdown,
        core,
        "billing",
        Duration::ZERO,
        HOUR,
        |core| async move { billing::reconcile_subscriptions(&core).await },
    );
    every(
        tracker,
        shutdown,
        core,
        "domain verification",
        Duration::ZERO,
        Duration::from_secs(30),
        |core| async move { domains::verify_custom_domains(&core).await },
    );
    every(
        tracker,
        shutdown,
        core,
        "domain reverification",
        3 * HOUR,
        3 * HOUR,
        |core| async move { domains::reverify_custom_domains(&core).await },
    );
    every(
        tracker,
        shutdown,
        core,
        "session sweep",
        HOUR,
        24 * HOUR,
        |core| async move { sessions::sweep(&core).await.map(drop) },
    );
    every(
        tracker,
        shutdown,
        core,
        "expiration sweep",
        MINUTE,
        MINUTE,
        |core| async move { events::sweep_expired_events(&core).await },
    );
    every(
        tracker,
        shutdown,
        core,
        "membership sweep",
        Duration::ZERO,
        5 * MINUTE,
        |core| async move { memberships::sweep(&core).await },
    );
    every(
        tracker,
        shutdown,
        core,
        "erasure",
        MINUTE,
        10 * MINUTE,
        |core| async move { hosting::erase_purged_relays(&core).await },
    );

    tracker.spawn(publish_robot_identity(core.clone(), shutdown.clone()));
}

/// Run `job` after `first`, then every `period`, logging its failures.
fn every<F, Fut>(
    tracker: &TaskTracker,
    shutdown: &CancellationToken,
    core: &Core,
    name: &'static str,
    first: Duration,
    period: Duration,
    job: F,
) where
    F: Fn(Core) -> Fut + Send + 'static,
    Fut: Future<Output = Result<()>> + Send,
{
    let core = core.clone();
    let shutdown = shutdown.clone();

    tracker.spawn(async move {
        let mut ticks = interval_at(Instant::now() + first, period);
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = ticks.tick() => {
                    if let Err(error) = job(core.clone()).await {
                        tracing::error!(worker = name, error = ?error, "worker failed");
                    }
                }
            }
        }
    });
}

/// Publish the robot's profile and relay lists, retrying with backoff until it
/// works, so a relay outage at startup doesn't hold up serving.
async fn publish_robot_identity(core: Core, shutdown: CancellationToken) {
    let mut delay = Duration::from_secs(5);

    loop {
        match core.robot.publish_identity(core.clock.now()).await {
            Ok(()) => return,
            Err(error) => tracing::warn!(error = %error, "failed to publish robot identity"),
        }

        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = tokio::time::sleep(delay) => {}
        }
        delay = (delay * 2).min(HOUR);
    }
}
