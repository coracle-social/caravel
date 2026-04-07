use anyhow::Result;

use crate::command::Command;
use crate::models::Activity;
use crate::query::Query;
use crate::robot::Robot;

#[derive(Clone)]
pub struct Billing {
    nwc_url: String,
    query: Query,
    command: Command,
    robot: Robot,
}

impl Billing {
    pub fn new(query: Query, command: Command, robot: Robot) -> Self {
        let nwc_url = std::env::var("NWC_URL").unwrap_or_default();
        Self {
            nwc_url,
            query,
            command,
            robot,
        }
    }

    pub async fn start(self) {
        let mut rx = self.command.notify.subscribe();

        loop {
            match rx.recv().await {
                Ok(activity) => {
                    if let Err(e) = self.handle_activity(&activity).await {
                        tracing::error!(error = %e, "billing handle_activity failed");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(missed = n, "billing lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }

    async fn handle_activity(&self, activity: &Activity) -> Result<()> {
        let needs_billing_sync = matches!(
            activity.activity_type.as_str(),
            "create_relay" | "update_relay" | "activate_relay" | "deactivate_relay"
                | "fail_relay_sync" | "complete_relay_sync"
        );

        if needs_billing_sync {
            self.sync_relay_subscription_item(activity).await?;
        }

        Ok(())
    }

    async fn sync_relay_subscription_item(&self, activity: &Activity) -> Result<()> {
        let Some(relay) = self.query.get_relay(&activity.resource_id).await? else {
            return Ok(());
        };

        let should_delete = !relay.sync_error.is_empty()
            || relay.synced == 0
            || relay.plan == "free"
            || relay.status == "inactive";

        if should_delete {
            if relay.stripe_subscription_item_id.is_some() {
                // TODO: Delete subscription item via Stripe API
                self.command.delete_relay_subscription_item(&relay.id).await?;
            }
        } else {
            // TODO: Create or update subscription item via Stripe API
            // let stripe_subscription_item_id = ...;
            // self.command.set_relay_subscription_item(&relay.id, &stripe_subscription_item_id).await?;
        }

        Ok(())
    }

    pub async fn deactivate_relay(&self, relay_id: &str) -> Result<()> {
        let relay = self
            .query
            .get_relay(relay_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("relay not found"))?;
        self.command.deactivate_relay(&relay).await
    }

    pub async fn reactivate_relay(&self, relay_id: &str) -> Result<()> {
        let relay = self
            .query
            .get_relay(relay_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("relay not found"))?;
        self.command.activate_relay(&relay).await
    }
}
