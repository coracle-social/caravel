//! The robot: the platform's own Nostr identity, which publishes its profile and
//! relay lists and sends tenants direct messages.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use coracle_kinds::gift_wrap::{GiftWrapWriter, SealWriter, seal_plaintext};
use coracle_kinds::profile::ProfileWriter;
use coracle_kinds::relay_list::RelayListWriter;
use coracle_lib::events::{Event, EventContent, HashedEvent, StampedEvent};
use coracle_lib::filters::Filter;
use coracle_lib::keys::{PublicKey, SecretKey};
use coracle_lib::tags::Tags;
use coracle_lib::writers::Writer;
use tokio::sync::Mutex;

use crate::nostr::Client;

/// How long to wait for relays to answer a lookup.
const FETCH_TIMEOUT: Duration = Duration::from_secs(5);

/// How long to wait for relays to take an event.
const PUBLISH_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a recipient's relay lists are trusted.
const CACHE_TTL: Duration = Duration::from_secs(300);

const PROFILE: u16 = 0;
const DIRECT_MESSAGE: u16 = 14;
const RELAY_LIST: u16 = 10002;
const MESSAGING_RELAYS: u16 = 10050;

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
/// lists and sends NIP-17 direct messages, caching recipients' relay lists
/// between sends.
#[derive(Clone)]
pub struct Robot {
    secret: SecretKey,
    client: Client,
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
        let secret = SecretKey::from_hex(secret)?;
        Ok(Self {
            client: Client::new(secret.clone()),
            secret,
            profile: Arc::new(profile),
            outbox_cache: Default::default(),
            dm_cache: Default::default(),
        })
    }

    /// Publish the robot's profile, outbox relays, and messaging relays, so
    /// tenants' clients can find and trust the DMs it sends.
    pub async fn publish_identity(&self, now: i64) -> Result<()> {
        let profile = &self.profile;

        let mut metadata = ProfileWriter::new();
        if !profile.name.is_empty() {
            metadata = metadata.with_name(&profile.name);
        }
        if !profile.description.is_empty() {
            metadata = metadata.with_about(&profile.description);
        }
        if !profile.picture.is_empty() {
            url::Url::parse(&profile.picture)?;
            metadata = metadata.with_picture(&profile.picture);
        }
        let metadata = self.sign(metadata.render_template()?.with_created_at(now));
        self.publish(&profile.outbox_relays, &metadata, now).await?;

        let outbox = profile
            .outbox_relays
            .iter()
            .fold(RelayListWriter::new(), |list, url| {
                list.add_read_url(url).add_write_url(url)
            });
        let outbox = self.sign(outbox.render_template()?.with_created_at(now));
        self.publish(&profile.outbox_relays, &outbox, now).await?;

        let messaging = profile
            .messaging_relays
            .iter()
            .fold(Tags::new(), |tags, url| tags.add("relay", [url.as_str()]));
        let messaging = self.sign(
            EventContent::new()
                .with_tags(messaging)
                .with_kind(MESSAGING_RELAYS)
                .with_created_at(now),
        );
        self.publish(&profile.indexer_relays, &messaging, now).await
    }

    /// Send a NIP-17 direct message to a recipient over their messaging relays.
    pub async fn send_dm(&self, recipient: &str, message: &str, now: i64) -> Result<()> {
        let outbox = self
            .fetch_relay_list(
                &self.outbox_cache,
                recipient,
                &self.profile.indexer_relays,
                RELAY_LIST,
                "r",
                now,
            )
            .await?;
        if outbox.is_empty() {
            return Err(anyhow!("no outbox relays found for recipient"));
        }

        let dm_relays = self
            .fetch_relay_list(
                &self.dm_cache,
                recipient,
                &outbox,
                MESSAGING_RELAYS,
                "relay",
                now,
            )
            .await?;
        if dm_relays.is_empty() {
            return Err(anyhow!("no messaging relays found for recipient"));
        }

        let recipient = PublicKey::from_hex(recipient)?;
        let rumor = EventContent::new()
            .with_content(message)
            .with_tags(Tags::new().add("p", [recipient.to_hex()]))
            .with_kind(DIRECT_MESSAGE)
            .with_created_at(now)
            .with_pubkey(self.secret.public_key())
            .with_id();
        let ciphertext = self
            .secret
            .nip44_encrypt(&recipient, &seal_plaintext(&rumor))?;
        let seal = SealWriter::new()
            .with_ciphertext(ciphertext)
            .render_seal(self.secret.public_key())?;
        let wrap = GiftWrapWriter::new()
            .with_recipient(recipient)
            .with_seal(self.signed(seal))
            .render_wrap()?;

        self.publish(&dm_relays, &wrap, now).await
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
        now: i64,
    ) -> Result<Vec<String>> {
        if let Some(values) = get_cached(cache, pubkey).await {
            return Ok(values);
        }

        let filter = author_filter(PublicKey::from_hex(pubkey)?, kind);
        let events = self
            .client
            .fetch(relays, &filter, now, FETCH_TIMEOUT)
            .await?;
        let list: Vec<String> = events
            .into_iter()
            .max_by_key(|event| event.created_at)
            .map(|event| event.tags.values(tag).map(str::to_string).collect())
            .unwrap_or_default();

        set_cached(cache, pubkey, list.clone()).await;
        Ok(list)
    }

    /// The recipient's display name from their Nostr profile, if they have one.
    pub async fn fetch_nostr_name(&self, pubkey: &str, now: i64) -> Option<String> {
        let filter = Filter {
            limit: Some(1),
            ..author_filter(PublicKey::from_hex(pubkey).ok()?, PROFILE)
        };
        let events = self
            .client
            .fetch(&self.profile.indexer_relays, &filter, now, FETCH_TIMEOUT)
            .await
            .ok()?;
        let event = events.into_iter().max_by_key(|event| event.created_at)?;
        let content: serde_json::Value = serde_json::from_str(&event.content).ok()?;
        content
            .get("display_name")
            .or_else(|| content.get("name"))
            .and_then(|value| value.as_str())
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
    }

    async fn publish(&self, relays: &[String], event: &Event, now: i64) -> Result<()> {
        self.client
            .publish(relays, event, now, PUBLISH_TIMEOUT)
            .await
    }

    /// An event from the robot, signed.
    fn sign(&self, stamped: StampedEvent) -> Event {
        self.signed(stamped.with_pubkey(self.secret.public_key()).with_id())
    }

    fn signed(&self, hashed: HashedEvent) -> Event {
        let sig = self.secret.sign(&hashed.id.to_bytes());
        hashed.with_sig(sig)
    }
}

/// The events of one kind a pubkey wrote.
fn author_filter(author: PublicKey, kind: u16) -> Filter {
    Filter {
        authors: Some(BTreeSet::from([author])),
        kinds: Some(BTreeSet::from([kind])),
        ..Default::default()
    }
}

async fn get_cached(cache: &RelayListCache, key: &str) -> Option<Vec<String>> {
    cache
        .lock()
        .await
        .get(key)
        .filter(|entry| entry.fetched_at.elapsed() < CACHE_TTL)
        .map(|entry| entry.values.clone())
}

async fn set_cached(cache: &RelayListCache, key: &str, values: Vec<String>) {
    cache.lock().await.insert(
        key.to_string(),
        CacheEntry {
            values,
            fetched_at: Instant::now(),
        },
    );
}
