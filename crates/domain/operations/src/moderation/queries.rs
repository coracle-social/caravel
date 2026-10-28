//! Reading the moderation feed: the events and uploads every moderated relay saw,
//! newest first. Relays owned by admins aren't moderated, and events no relay serves,
//! or that run a relay rather than say anything, aren't shown.

use std::collections::HashMap;

use sqlx::{PgPool, Row};

use super::{ModerationItem, ModerationPage, Subject};
use caravel_relays::media::Upload;
use caravel_util::error::Result;
use caravel_util::nostr::kinds;

/// The most items one page holds.
const PAGE_SIZE: usize = 1000;

/// The items seen within an inclusive window of `seen_at`, newest first. Either
/// bound may be left open. Relays the `admins` own aren't moderated, and the events
/// that run a relay are left out: what the relay signs, room moderation and join
/// requests, and relay and room lists.
pub async fn list_moderation(
    pool: &PgPool,
    admins: &[String],
    since: Option<i64>,
    until: Option<i64>,
) -> Result<ModerationPage> {
    let hidden: Vec<i32> = caravel_relays::policy::UNREADABLE_KINDS
        .into_iter()
        .chain(kinds::rooms::PUT_USER..=kinds::rooms::LEAVE_REQUEST)
        .chain([kinds::RELAY_LIST, kinds::ROOM_LIST])
        .map(i32::from)
        .collect();
    let since = since.unwrap_or(0);
    let until = until.unwrap_or(i64::MAX);

    let events = sqlx::query(
        "SELECT e.relay_id, e.seen_at, e.id, e.pubkey, e.created_at, e.kind, e.tag_values, e.content, e.tags, e.sig
         FROM event e JOIN relay r ON r.id = e.relay_id
         WHERE r.tenant_pubkey <> ALL($1) AND e.pubkey <> decode(r.pubkey, 'hex')
           AND e.kind <> ALL($2) AND e.seen_at BETWEEN $3 AND $4
         ORDER BY e.seen_at DESC, e.seq DESC LIMIT $5",
    )
    .bind(admins)
    .bind(&hidden)
    .bind(since)
    .bind(until)
    .bind(PAGE_SIZE as i64 + 1)
    .fetch_all(pool)
    .await?;

    let uploads = sqlx::query(
        "SELECT o.relay_id, o.created_at AS seen_at, o.pubkey, b.sha256, b.type, b.size
         FROM blob_owner o
         JOIN blob b ON b.relay_id = o.relay_id AND b.sha256 = o.blob_sha256
         JOIN relay r ON r.id = o.relay_id
         WHERE r.tenant_pubkey <> ALL($1) AND o.created_at BETWEEN $2 AND $3
         ORDER BY o.created_at DESC LIMIT $4",
    )
    .bind(admins)
    .bind(since)
    .bind(until)
    .bind(PAGE_SIZE as i64 + 1)
    .fetch_all(pool)
    .await?;

    let hosts: HashMap<String, String> = caravel_billing::relays::queries::list_relays(pool)
        .await?
        .into_iter()
        .map(|relay| (relay.id.clone(), relay.host()))
        .collect();

    let mut items = Vec::with_capacity(events.len() + uploads.len());
    for row in &events {
        let Some(stored) = caravel_relays::events::queries::stored_event(row) else {
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
