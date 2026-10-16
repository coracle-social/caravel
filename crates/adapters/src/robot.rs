//! The robot: the platform's own Nostr identity, which publishes its profile and
//! relay lists and sends tenants direct messages.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use nostr_sdk::prelude::*;
use tokio::sync::Mutex;

/// The robot's identity and where it publishes, as the app configures it.
#[derive(Clone, Debug, Default)]
pub struct RobotProfile {
    pub name: String,
    pub picture: String,
    pub description: String,
    pub outbox_relays: Vec<String>,
    pub indexer_relays: Vec<String>,
    pub messaging_relays: Vec<String>,
}

/// The platform's Nostr identity: it publishes the robot's profile and relay
/// lists and sends encrypted direct messages, caching recipients' relay lists
/// between sends.
#[derive(Clone)]
pub struct Robot {
    keys: Keys,
    profile: Arc<RobotProfile>,
    outbox_cache: RelayListCache,
    dm_cache: RelayListCache,
}

/// Pubkeys' relay lists of one kind, as last fetched.
type RelayListCache = Arc<Mutex<HashMap<String, CacheEntry>>>;

#[derive(Clone)]
struct CacheEntry {
    values: Vec<String>,
    fetched_at: Instant,
}

impl Robot {
    /// Build the robot from its hex secret key. Does no I/O; call
    /// [`Robot::publish_identity`] to announce it before relying on it to send DMs.
    pub fn new(secret: &str, profile: RobotProfile) -> Result<Self> {
        Ok(Self {
            keys: Keys::parse(secret)?,
            profile: Arc::new(profile),
            outbox_cache: Default::default(),
            dm_cache: Default::default(),
        })
    }

    async fn make_client(&self, relays: &[String]) -> Result<Client> {
        let client = Client::new(self.keys.clone());
        for relay in relays {
            client.add_relay(relay).await?;
        }
        client.connect().await;
        Ok(client)
    }

    /// Publish the robot's profile, outbox relays, and messaging relays, so
    /// tenants' clients can find and trust the DMs it sends.
    pub async fn publish_identity(&self) -> Result<()> {
        let profile = &self.profile;
        let mut metadata = Metadata::new();
        if !profile.name.is_empty() {
            metadata = metadata.name(&profile.name);
        }
        if !profile.description.is_empty() {
            metadata = metadata.about(&profile.description);
        }
        if !profile.picture.is_empty() {
            metadata = metadata.picture(Url::parse(&profile.picture)?);
        }

        let outbox_client = self.make_client(&profile.outbox_relays).await?;
        let indexer_client = self.make_client(&profile.indexer_relays).await?;

        outbox_client
            .send_event_builder(EventBuilder::metadata(&metadata))
            .await?;

        let outbox_tags = profile
            .outbox_relays
            .iter()
            .map(|r| Tag::parse(["r", r.as_str()]))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        outbox_client
            .send_event_builder(EventBuilder::new(Kind::Custom(10002), "").tags(outbox_tags))
            .await?;

        let messaging_tags = profile
            .messaging_relays
            .iter()
            .map(|r| Tag::parse(["relay", r.as_str()]))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        indexer_client
            .send_event_builder(EventBuilder::new(Kind::Custom(10050), "").tags(messaging_tags))
            .await?;

        Ok(())
    }

    /// Send an encrypted direct message to a recipient over their messaging relays.
    pub async fn send_dm(&self, recipient: &str, message: &str) -> Result<()> {
        let outbox = self
            .fetch_relay_list(
                &self.outbox_cache,
                recipient,
                &self.profile.indexer_relays,
                10002,
                "r",
            )
            .await?;
        if outbox.is_empty() {
            return Err(anyhow!("no outbox relays found for recipient"));
        }

        let dm_relays = self
            .fetch_relay_list(&self.dm_cache, recipient, &outbox, 10050, "relay")
            .await?;
        if dm_relays.is_empty() {
            return Err(anyhow!("no messaging relays found for recipient"));
        }

        let recipient_pubkey = PublicKey::parse(recipient)?;
        let client = self.make_client(&dm_relays).await?;
        client
            .send_private_msg(recipient_pubkey, message, [])
            .await?;
        Ok(())
    }

    /// The relays a pubkey's latest relay list of `kind`, looked up on `relays`,
    /// names in its `tag` tags, cached for five minutes.
    async fn fetch_relay_list(
        &self,
        cache: &RelayListCache,
        pubkey: &str,
        relays: &[String],
        kind: u16,
        tag: &str,
    ) -> Result<Vec<String>> {
        if let Some(values) = get_cached(cache, pubkey).await {
            return Ok(values);
        }

        let author = PublicKey::parse(pubkey)?;
        let filter = Filter::new().author(author).kind(Kind::Custom(kind));
        let client = self.make_client(relays).await?;
        let events = client.fetch_events(filter, Duration::from_secs(5)).await?;

        let mut list = Vec::new();
        if let Some(event) = events.into_iter().max_by_key(|e| e.created_at) {
            for values in event.tags.iter().map(|tag| tag.as_slice()) {
                if values.len() >= 2 && values[0] == tag {
                    list.push(values[1].to_string());
                }
            }
        }

        set_cached(cache, pubkey, list.clone()).await;
        Ok(list)
    }

    /// The recipient's display name from their Nostr profile, if they have one.
    pub async fn fetch_nostr_name(&self, pubkey: &str) -> Option<String> {
        let pubkey = PublicKey::parse(pubkey).ok()?;
        let filter = Filter::new().author(pubkey).kind(Kind::Metadata).limit(1);
        let client = self.make_client(&self.profile.indexer_relays).await.ok()?;
        let events = client
            .fetch_events(filter, Duration::from_secs(5))
            .await
            .ok()?;
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
}

async fn get_cached(cache: &RelayListCache, key: &str) -> Option<Vec<String>> {
    let guard = cache.lock().await;
    guard.get(key).and_then(|entry| {
        if entry.fetched_at.elapsed() < Duration::from_secs(300) {
            Some(entry.values.clone())
        } else {
            None
        }
    })
}

async fn set_cached(cache: &RelayListCache, key: &str, values: Vec<String>) {
    let mut guard = cache.lock().await;
    guard.insert(
        key.to_string(),
        CacheEntry {
            values,
            fetched_at: Instant::now(),
        },
    );
}
