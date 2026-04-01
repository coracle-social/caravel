use anyhow::Result;

use crate::command::Command;
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
