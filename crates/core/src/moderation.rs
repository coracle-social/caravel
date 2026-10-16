//! The moderation feed: the events and uploads every moderated relay saw, newest
//! first, for admins to review. Relays owned by admins aren't moderated, and
//! events no relay serves aren't shown.

use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;
use sqlx::Row;

use crate::Core;
use crate::error::Result;
use crate::events;
use crate::relay_policy::UNREADABLE_KINDS;
use crate::relays;

/// The most items one page holds.
const PAGE_SIZE: usize = 1000;

/// One reviewable item, with the relay it came from and when that relay first
/// stored it.
#[derive(Clone, Debug, Serialize)]
pub struct ModerationItem {
    pub relay_id: String,
    pub seen_at: i64,
    #[serde(flatten)]
    pub subject: Subject,
}

/// What is up for review.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "lowercase")]
pub enum Subject {
    /// An event, as NIP-01 JSON.
    Event(Value),
    Upload(Upload),
}

/// A blob a member uploaded.
#[derive(Clone, Debug, Serialize)]
pub struct Upload {
    pub sha256: String,
    /// Where the relay serves it.
    pub url: String,
    #[serde(rename = "type")]
    pub content_type: String,
    pub size: i64,
    /// The uploader.
    pub pubkey: String,
    pub created_at: i64,
}

/// One page of the feed, and whether its window held more than fit.
#[derive(Clone, Debug, Serialize)]
pub struct ModerationPage {
    pub items: Vec<ModerationItem>,
    pub truncated: bool,
}

/// The items seen within an inclusive window of `seen_at`, newest first. Either
/// bound may be left open.
pub async fn list_moderation(
    core: &Core,
    since: Option<i64>,
    until: Option<i64>,
) -> Result<ModerationPage> {
    let admins = core.config.admin_pubkeys.clone();
    let hidden: Vec<i32> = UNREADABLE_KINDS.into_iter().map(i32::from).collect();
    let since = since.unwrap_or(0);
    let until = until.unwrap_or(i64::MAX);

    let events = sqlx::query(
        "SELECT e.relay_id, e.seen_at, e.id, e.pubkey, e.created_at, e.kind, e.tag_values, e.content, e.tags, e.sig
         FROM event e JOIN relay r ON r.id = e.relay_id
         WHERE r.tenant_pubkey <> ALL($1) AND e.kind <> ALL($2) AND e.seen_at BETWEEN $3 AND $4
         ORDER BY e.seen_at DESC, e.seq DESC LIMIT $5",
    )
    .bind(&admins)
    .bind(&hidden)
    .bind(since)
    .bind(until)
    .bind(PAGE_SIZE as i64 + 1)
    .fetch_all(core.db.pool())
    .await?;

    let uploads = sqlx::query(
        "SELECT o.relay_id, o.created_at AS seen_at, o.pubkey, b.sha256, b.type, b.size
         FROM blob_owner o
         JOIN blob b ON b.relay_id = o.relay_id AND b.sha256 = o.blob_sha256
         JOIN relay r ON r.id = o.relay_id
         WHERE r.tenant_pubkey <> ALL($1) AND o.created_at BETWEEN $2 AND $3
         ORDER BY o.created_at DESC LIMIT $4",
    )
    .bind(&admins)
    .bind(since)
    .bind(until)
    .bind(PAGE_SIZE as i64 + 1)
    .fetch_all(core.db.pool())
    .await?;

    let hosts: HashMap<String, String> = relays::list_relays(core.db.pool())
        .await?
        .into_iter()
        .map(|relay| (relay.id.clone(), relay.host()))
        .collect();

    let mut items = Vec::with_capacity(events.len() + uploads.len());
    for row in &events {
        let Some(stored) = events::stored_event(row) else {
            continue;
        };
        items.push(ModerationItem {
            relay_id: row.get("relay_id"),
            seen_at: row.get("seen_at"),
            subject: Subject::Event(
                serde_json::from_str(&stored.to_json(false)).map_err(anyhow::Error::from)?,
            ),
        });
    }
    for row in &uploads {
        let relay_id: String = row.get("relay_id");
        let sha256: String = row.get("sha256");
        let seen_at: i64 = row.get("seen_at");
        let host = hosts.get(&relay_id).cloned().unwrap_or_default();
        items.push(ModerationItem {
            subject: Subject::Upload(Upload {
                url: format!("https://{host}/{sha256}"),
                sha256,
                content_type: row.get("type"),
                size: row.get("size"),
                pubkey: row.get("pubkey"),
                created_at: seen_at,
            }),
            relay_id,
            seen_at,
        });
    }

    items.sort_by_key(|item| std::cmp::Reverse(item.seen_at));
    let truncated = items.len() > PAGE_SIZE;
    items.truncate(PAGE_SIZE);

    Ok(ModerationPage { items, truncated })
}
