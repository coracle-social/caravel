use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use nostr_sdk::prelude::*;
use tokio::sync::Mutex;

use crate::env::Env;

#[derive(Clone)]
pub struct Robot {
    env: Env,
    outbox_cache: std::sync::Arc<Mutex<HashMap<String, CacheEntry>>>,
    dm_cache: std::sync::Arc<Mutex<HashMap<String, CacheEntry>>>,
}

#[derive(Clone)]
struct CacheEntry {
    values: Vec<String>,
    fetched_at: Instant,
}

impl Robot {
    pub async fn new(env: &Env) -> Result<Self> {
        let robot = Self {
            env: env.clone(),
            outbox_cache: std::sync::Arc::new(Mutex::new(HashMap::new())),
            dm_cache: std::sync::Arc::new(Mutex::new(HashMap::new())),
        };

        robot.publish_identity().await?;
        Ok(robot)
    }

    async fn make_client(&self, relays: &[String]) -> Result<Client> {
        let client = Client::new(self.env.keys.clone());
        for relay in relays {
            client.add_relay(relay).await?;
        }
        client.connect().await;
        Ok(client)
    }


    async fn publish_identity(
        &self,
    ) -> Result<()> {
        let mut metadata = Metadata::new();
        if !self.env.robot_name.is_empty() {
            metadata = metadata.name(&self.env.robot_name);
        }
        if !self.env.robot_description.is_empty() {
            metadata = metadata.about(&self.env.robot_description);
        }
        if !self.env.robot_picture.is_empty() {
            metadata = metadata.picture(Url::parse(&self.env.robot_picture)?);
        }

        let outbox_client = self.make_client(&self.env.robot_outbox_relays).await?;
        let indexer_client = self.make_client(&self.env.robot_indexer_relays).await?;

        outbox_client
            .send_event_builder(EventBuilder::metadata(&metadata))
            .await?;

        let outbox_tags = self.env.robot_outbox_relays
            .iter()
            .map(|r| Tag::parse(["r", r.as_str()]))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        outbox_client
            .send_event_builder(EventBuilder::new(Kind::Custom(10002), "").tags(outbox_tags))
            .await?;

        let messaging_tags = self.env.robot_messaging_relays
            .iter()
            .map(|r| Tag::parse(["relay", r.as_str()]))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        indexer_client
            .send_event_builder(EventBuilder::new(Kind::Custom(10050), "").tags(messaging_tags))
            .await?;

        Ok(())
    }

    pub async fn send_dm(&self, recipient: &str, message: &str) -> Result<()> {
        let outbox = self.fetch_outbox_relays(recipient).await?;
        if outbox.is_empty() {
            return Err(anyhow!("no outbox relays found for recipient"));
        }

        let dm_relays = self
            .fetch_messaging_relays_from_outbox(recipient, &outbox)
            .await?;
        if dm_relays.is_empty() {
            return Err(anyhow!("no messaging relays found for recipient"));
        }

        let recipient_pubkey = PublicKey::parse(recipient)?;
        let client = self.make_client(&dm_relays).await?;
        client.send_private_msg(recipient_pubkey, message, []).await?;
        Ok(())
    }

    async fn fetch_outbox_relays(&self, recipient: &str) -> Result<Vec<String>> {
        if let Some(values) = get_cached(&self.outbox_cache, recipient).await {
            return Ok(values);
        }

        let pubkey = PublicKey::parse(recipient)?;
        let filter = Filter::new().author(pubkey).kind(Kind::Custom(10002));
        let client = self.make_client(&self.env.robot_indexer_relays).await?;
        let events = client.fetch_events(filter, Duration::from_secs(5)).await?;

        let mut relays = Vec::new();
        if let Some(event) = events.into_iter().max_by_key(|e| e.created_at) {
            for tag in event.tags.iter() {
                let values = tag.as_slice();
                if values.len() >= 2 && values[0] == "r" {
                    relays.push(values[1].to_string());
                }
            }
        }

        set_cached(&self.outbox_cache, recipient, relays.clone()).await;
        Ok(relays)
    }

    pub async fn fetch_nostr_name(&self, pubkey: &str) -> Option<String> {
        let pubkey = PublicKey::parse(pubkey).ok()?;
        let filter = Filter::new().author(pubkey).kind(Kind::Metadata).limit(1);
        let client = self.make_client(&self.env.robot_indexer_relays).await.ok()?;
        let events = client.fetch_events(filter, Duration::from_secs(5)).await.ok()?;
        let event = events.into_iter().max_by_key(|e| e.created_at)?;
        let content: serde_json::Value = serde_json::from_str(&event.content).ok()?;
        let name = content
            .get("display_name")
            .or_else(|| content.get("name"))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())?;
        Some(name)
    }

    async fn fetch_messaging_relays_from_outbox(
        &self,
        recipient: &str,
        outbox_relays: &[String],
    ) -> Result<Vec<String>> {
        if let Some(values) = get_cached(&self.dm_cache, recipient).await {
            return Ok(values);
        }

        let pubkey = PublicKey::parse(recipient)?;
        let client = self.make_client(outbox_relays).await?;
        let filter = Filter::new().author(pubkey).kind(Kind::Custom(10050));
        let events = client.fetch_events(filter, Duration::from_secs(5)).await?;

        let mut relays = Vec::new();
        if let Some(event) = events.into_iter().max_by_key(|e| e.created_at) {
            for tag in event.tags.iter() {
                let values = tag.as_slice();
                if values.len() >= 2 && values[0] == "relay" {
                    relays.push(values[1].to_string());
                }
            }
        }

        set_cached(&self.dm_cache, recipient, relays.clone()).await;
        Ok(relays)
    }
}

async fn get_cached(
    cache: &std::sync::Arc<Mutex<HashMap<String, CacheEntry>>>,
    key: &str,
) -> Option<Vec<String>> {
    let guard = cache.lock().await;
    guard.get(key).and_then(|entry| {
        if entry.fetched_at.elapsed() < Duration::from_secs(300) {
            Some(entry.values.clone())
        } else {
            None
        }
    })
}

async fn set_cached(
    cache: &std::sync::Arc<Mutex<HashMap<String, CacheEntry>>>,
    key: &str,
    values: Vec<String>,
) {
    let mut guard = cache.lock().await;
    guard.insert(
        key.to_string(),
        CacheEntry {
            values,
            fetched_at: Instant::now(),
        },
    );
}
