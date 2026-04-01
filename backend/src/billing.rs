use anyhow::Result;

use crate::repo::Repo;
use crate::robot::Robot;

#[derive(Clone)]
pub struct Billing {
    nwc_url: String,
    repo: Repo,
    robot: Robot,
}

impl Billing {
    pub fn new(repo: Repo, robot: Robot) -> Self {
        let nwc_url = std::env::var("NWC_URL").unwrap_or_default();
        Self {
            nwc_url,
            repo,
            robot,
        }
    }

    pub async fn deactivate_relay(&self, relay_id: &str) -> Result<()> {
        let relay = self
            .repo
            .get_relay(relay_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("relay not found"))?;
        self.repo.deactivate_relay(&relay).await
    }

    pub async fn reactivate_relay(&self, relay_id: &str) -> Result<()> {
        let relay = self
            .repo
            .get_relay(relay_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("relay not found"))?;
        self.repo.activate_relay(&relay).await
    }
}
