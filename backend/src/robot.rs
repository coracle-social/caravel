use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use nostr_sdk::prelude::*;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct Robot {
    secret: String,
    name: String,
    description: String,
    picture: String,
    outbox_relays: Vec<String>,
    indexer_relays: Vec<String>,
    messaging_relays: Vec<String>,
    client: Client,
    outbox_cache: std::sync::Arc<Mutex<HashMap<String, CacheEntry>>>,
    dm_cache: std::sync::Arc<Mutex<HashMap<String, CacheEntry>>>,
}

#[derive(Clone)]
struct CacheEntry {
    values: Vec<String>,
    fetched_at: Instant,
}

impl Robot {
    pub async fn new() -> Result<Self> {
        let secret = std::env::var("ROBOT_SECRET").unwrap_or_default();
        if secret.trim().is_empty() {
            return Err(anyhow!("ROBOT_SECRET is required"));
        }

        let name = std::env::var("ROBOT_NAME").unwrap_or_default();
        let description = std::env::var("ROBOT_DESCRIPTION").unwrap_or_default();
        let picture = std::env::var("ROBOT_PICTURE").unwrap_or_default();
        let outbox_relays = split_env("ROBOT_OUTBOX_RELAYS");
        let indexer_relays = split_env("ROBOT_INDEXER_RELAYS");
        let messaging_relays = split_env("ROBOT_MESSAGING_RELAYS");

        let keys = Keys::parse(&secret)?;
        let client = Client::new(keys);
        for relay in &outbox_relays {
            client.add_relay(relay).await?;
        }
        for relay in &indexer_relays {
            client.add_relay(relay).await?;
        }
        for relay in &messaging_relays {
            client.add_relay(relay).await?;
        }
        client.connect().await;

        let robot = Self {
            secret,
            name,
            description,
            picture,
            outbox_relays,
            indexer_relays,
            messaging_relays,
            client,
            outbox_cache: std::sync::Arc::new(Mutex::new(HashMap::new())),
            dm_cache: std::sync::Arc::new(Mutex::new(HashMap::new())),
        };

        robot.publish_identity().await?;
        Ok(robot)
    }

    async fn publish_identity(&self) -> Result<()> {
        let mut metadata = Metadata::new();
        if !self.name.is_empty() {
            metadata = metadata.name(&self.name);
        }
        if !self.description.is_empty() {
            metadata = metadata.about(&self.description);
        }
        if !self.picture.is_empty() {
            metadata = metadata.picture(Url::parse(&self.picture)?);
        }

        self.client
            .send_event_builder(EventBuilder::metadata(&metadata))
            .await?;

        let outbox_tags = self
            .outbox_relays
            .iter()
            .map(|r| Tag::parse(["r", r.as_str()]))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        self.client
            .send_event_builder(EventBuilder::new(Kind::Custom(10002), "").tags(outbox_tags))
            .await?;

        let mut selection_tags = Vec::new();
        for relay in &self.messaging_relays {
            selection_tags.push(Tag::parse(["relay", relay.as_str()])?);
        }
        self.client
            .send_event_builder(EventBuilder::new(Kind::Custom(10050), "").tags(selection_tags))
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
        let keys = Keys::parse(&self.secret)?;
        let client = Client::new(keys);
        for relay in dm_relays {
            client.add_relay(relay).await?;
        }
        client.connect().await;
        client
            .send_private_msg(recipient_pubkey, message, [])
            .await?;
        Ok(())
    }

    async fn fetch_outbox_relays(&self, recipient: &str) -> Result<Vec<String>> {
        if let Some(values) = get_cached(&self.outbox_cache, recipient).await {
            return Ok(values);
        }

        let pubkey = PublicKey::parse(recipient)?;
        let client = indexer_client(&self.secret, &self.indexer_relays).await?;
        let filter = Filter::new().author(pubkey).kind(Kind::Custom(10002));
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

    async fn fetch_messaging_relays_from_outbox(
        &self,
        recipient: &str,
        outbox_relays: &[String],
    ) -> Result<Vec<String>> {
        if let Some(values) = get_cached(&self.dm_cache, recipient).await {
            return Ok(values);
        }

        let pubkey = PublicKey::parse(recipient)?;
        let keys = Keys::parse(&self.secret)?;
        let client = Client::new(keys);
        for relay in outbox_relays {
            client.add_relay(relay).await?;
        }
        client.connect().await;

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

fn split_env(key: &str) -> Vec<String> {
    std::env::var(key)
        .unwrap_or_default()
        .split(',')
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect()
}

async fn indexer_client(secret: &str, indexer_relays: &[String]) -> Result<Client> {
    let keys = Keys::parse(secret)?;
    let client = Client::new(keys);
    for relay in indexer_relays {
        client.add_relay(relay).await?;
    }
    client.connect().await;
    Ok(client)
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
