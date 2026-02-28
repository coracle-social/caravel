use anyhow::{Result, anyhow};
use nostr_sdk::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct Nip17Notifier {
    keys: Keys,
    indexer_client: Client,
    indexer_enabled: bool,
    cache: Arc<Mutex<HashMap<String, CacheEntry>>>,
}

impl Nip17Notifier {
    pub async fn new(platform_secret: String, relays: Vec<String>) -> Result<Self> {
        if platform_secret.trim().is_empty() {
            return Err(anyhow!(
                "PLATFORM_SECRET is required for NIP-17 notifications"
            ));
        }

        let keys = Keys::parse(&platform_secret)?;
        let indexer_client = Client::new(keys.clone());

        for relay in &relays {
            indexer_client.add_relay(relay).await?;
        }

        let indexer_enabled = !relays.is_empty();
        if indexer_enabled {
            indexer_client.connect().await;
        }

        Ok(Self {
            keys,
            indexer_client,
            indexer_enabled,
            cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn send(&self, recipient: &str, message: &str) -> Result<()> {
        if !self.indexer_enabled {
            return Ok(());
        }

        let relays = self.fetch_dm_relays(recipient).await?;
        if relays.is_empty() {
            return Ok(());
        }

        let pubkey = PublicKey::parse(recipient)?;
        let client = Client::new(self.keys.clone());
        for relay in relays {
            client.add_relay(relay).await?;
        }
        client.connect().await;
        client.send_private_msg(pubkey, message, []).await?;
        Ok(())
    }

    async fn fetch_dm_relays(&self, recipient: &str) -> Result<Vec<String>> {
        let mut cache = self.cache.lock().await;
        if let Some(entry) = cache.get(recipient) {
            if entry.fetched_at.elapsed() < Duration::from_secs(300) {
                return Ok(entry.relays.clone());
            }
        }

        let pubkey = PublicKey::parse(recipient)?;
        let filter = Filter::new().kind(Kind::Custom(10050)).author(pubkey);
        let events = self
            .indexer_client
            .fetch_events(filter, Duration::from_secs(5))
            .await?;

        let mut relays = Vec::new();
        if let Some(event) = events.into_iter().max_by_key(|event| event.created_at) {
            for tag in event.tags.iter() {
                if let Some(first) = tag.as_slice().get(0) {
                    if first == "relay" {
                        if let Some(value) = tag.as_slice().get(1) {
                            relays.push(value.to_string());
                        }
                    }
                }
            }
        }

        cache.insert(
            recipient.to_string(),
            CacheEntry {
                relays: relays.clone(),
                fetched_at: Instant::now(),
            },
        );

        Ok(relays)
    }
}

#[derive(Clone)]
struct CacheEntry {
    relays: Vec<String>,
    fetched_at: Instant,
}
