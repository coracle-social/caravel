//! The moderation feed: the events and uploads every moderated relay saw, newest
//! first, for admins to review. Relays owned by admins aren't moderated.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;

use crate::Core;
use crate::error::Result;
use crate::events::{EventHead, StoredEvent};
use crate::relay_policy::kinds;
use crate::rooms;

/// The most items one page holds.
const PAGE_SIZE: usize = 1000;

/// One reviewable item: an event, or an upload described as a blob, with the
/// relay it came from and when that relay first stored it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModerationItem {
    #[serde(rename = "type")]
    pub item_type: String,
    pub relay_id: String,
    pub seen_at: i64,
    pub data: Value,
}

/// One page of the feed, and whether its window held more than fit.
#[derive(Clone, Debug, Serialize, Deserialize)]
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
    let hidden: Vec<i32> = [
        kinds::RELAY_INVITE,
        rooms::kinds::CREATE_INVITE,
        kinds::RELAY_JOIN,
        kinds::RELAY_LEAVE,
        kinds::PUSH_SUBSCRIPTION,
    ]
    .into_iter()
    .map(i32::from)
    .collect();
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
        "SELECT o.relay_id, o.created_at AS seen_at, o.pubkey, b.sha256, b.type, b.size,
                r.subdomain, r.domain, r.custom_domain, r.custom_domain_verified
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

    let mut items = Vec::with_capacity(events.len() + uploads.len());
    for row in &events {
        let stored = StoredEvent {
            head: EventHead {
                id: row.get::<Vec<u8>, _>("id").try_into().unwrap_or_default(),
                pubkey: row
                    .get::<Vec<u8>, _>("pubkey")
                    .try_into()
                    .unwrap_or_default(),
                created_at: row.get("created_at"),
                kind: u16::try_from(row.get::<i32, _>("kind")).unwrap_or_default(),
                tag_values: row.get("tag_values"),
            },
            content: row.get("content"),
            tags: row.get("tags"),
            sig: row.get::<Vec<u8>, _>("sig").try_into().unwrap_or([0; 64]),
        };
        items.push(ModerationItem {
            item_type: "event".to_string(),
            relay_id: row.get("relay_id"),
            seen_at: row.get("seen_at"),
            data: serde_json::from_str(&stored.to_json(false)).map_err(anyhow::Error::from)?,
        });
    }
    for row in &uploads {
        let custom_domain: String = row.get("custom_domain");
        let host = if row.get::<bool, _>("custom_domain_verified") && !custom_domain.is_empty() {
            custom_domain
        } else {
            format!(
                "{}.{}",
                row.get::<String, _>("subdomain"),
                row.get::<String, _>("domain")
            )
        };
        let sha256: String = row.get("sha256");
        let seen_at: i64 = row.get("seen_at");
        items.push(ModerationItem {
            item_type: "upload".to_string(),
            relay_id: row.get("relay_id"),
            seen_at,
            data: json!({
                "sha256": sha256,
                "url": format!("https://{host}/{sha256}"),
                "type": row.get::<String, _>("type"),
                "size": row.get::<i64, _>("size"),
                "pubkey": row.get::<String, _>("pubkey"),
                "created_at": seen_at,
            }),
        });
    }

    items.sort_by_key(|item| std::cmp::Reverse(item.seen_at));
    let truncated = items.len() > PAGE_SIZE;
    items.truncate(PAGE_SIZE);

    Ok(ModerationPage { items, truncated })
}
