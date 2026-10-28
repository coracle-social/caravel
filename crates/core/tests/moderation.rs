//! The moderation feed, ported from zooid's API tests: every moderated relay's
//! events and uploads, newest first. The feed reads every relay, so each test
//! reads its own window of time.

mod common;

use anyhow::Result;
use caravel_core::testing::sign_event;
use caravel_core::{Core, testing};
use caravel_operations::moderation::Subject;
use caravel_util::nostr::kinds;
use common::*;
use coracle_lib::events::Event;
use coracle_lib::keys::SecretKey;

async fn save(core: &Core, relay_id: &str, events: &[Event]) -> Result<()> {
    let now = core.clock.now();
    core.db
        .with_tx(async |conn| {
            for event in events {
                caravel_relays::events::commands::save_event(&mut *conn, relay_id, event, now)
                    .await?;
            }
            Ok::<_, caravel_util::error::Error>(())
        })
        .await?;
    Ok(())
}

async fn upload(core: &Core, relay_id: &str, sha256: &str, pubkey: &str) -> Result<()> {
    let now = core.clock.now();
    core.db
        .with_tx(async |conn| {
            caravel_relays::media::commands::record_upload(
                conn,
                relay_id,
                sha256,
                "image/png",
                10,
                pubkey,
                now,
            )
            .await
        })
        .await?;
    Ok(())
}

#[tokio::test]
async fn the_feed_holds_moderated_events_and_uploads_newest_first() -> Result<()> {
    let core = core().await;
    let start = day(2030, 1, 1);
    let author = SecretKey::generate();

    let tenant = create_tenant(&core).await?;
    let relay = create_relay(&core, &tenant, "free").await?;
    testing::insert_tenant(&core, &admin_pubkey(&core)).await;
    let unmoderated = create_relay(&core, &admin_pubkey(&core), "free").await?;

    travel(&core, start);
    let note = sign_event(&author, 1, "note", &[], start);
    let join = sign_event(&author, kinds::RELAY_JOIN, "", &[], start);
    save(&core, &relay.id, &[note.clone(), join]).await?;
    save(
        &core,
        &unmoderated.id,
        &[sign_event(&author, 1, "admin's", &[], start)],
    )
    .await?;

    travel(&core, start + 10);
    upload(&core, &relay.id, "abc", &author.public_key().to_hex()).await?;
    upload(&core, &unmoderated.id, "def", &author.public_key().to_hex()).await?;

    let page = caravel_operations::moderation::queries::list_moderation(
        core.db.pool(),
        &core.config.admin_pubkeys,
        Some(start),
        Some(start + 10),
    )
    .await?;
    assert!(!page.truncated);
    let seen: Vec<i64> = page.items.iter().map(|item| item.seen_at).collect();
    assert_eq!(seen, vec![start + 10, start]);
    assert!(page.items.iter().all(|item| item.relay_id == relay.id));

    let Subject::Upload(upload) = &page.items[0].subject else {
        panic!("expected an upload first");
    };
    assert_eq!(upload.sha256, "abc");
    assert_eq!(
        upload.url,
        format!("https://{}/abc", relay.canonical_host())
    );
    let Subject::Event(event) = &page.items[1].subject else {
        panic!("expected an event second");
    };
    assert_eq!(event["id"], note.id.to_hex());
    Ok(())
}

/// Seen once per event id: republishing an event keeps its first time, while the
/// newer version that replaces it is seen when that version arrived.
#[tokio::test]
async fn an_event_is_seen_once_and_a_replacement_is_seen_as_it_arrives() -> Result<()> {
    let core = core().await;
    let start = day(2031, 6, 1);
    let author = SecretKey::generate();
    let tenant = create_tenant(&core).await?;
    let relay = create_relay(&core, &tenant, "free").await?;

    // A profile, which is replaceable, published and then published again.
    travel(&core, start);
    let first = sign_event(&author, 0, "before", &[], start);
    save(&core, &relay.id, std::slice::from_ref(&first)).await?;
    travel(&core, start + 60);
    save(&core, &relay.id, std::slice::from_ref(&first)).await?;

    let seen = |page: &caravel_operations::moderation::ModerationPage| -> Vec<i64> {
        page.items.iter().map(|item| item.seen_at).collect()
    };
    let page = caravel_operations::moderation::queries::list_moderation(
        core.db.pool(),
        &core.config.admin_pubkeys,
        Some(start),
        Some(start + 60),
    )
    .await?;
    assert_eq!(seen(&page), vec![start]);

    // The version that replaces it is a new event, seen when it arrived.
    travel(&core, start + 3600);
    save(
        &core,
        &relay.id,
        &[sign_event(&author, 0, "after", &[], start + 3600)],
    )
    .await?;
    let page = caravel_operations::moderation::queries::list_moderation(
        core.db.pool(),
        &core.config.admin_pubkeys,
        Some(start + 3600),
        Some(start + 3600),
    )
    .await?;
    assert_eq!(seen(&page), vec![start + 3600]);
    let Subject::Event(event) = &page.items[0].subject else {
        panic!("expected an event");
    };
    assert_eq!(event["content"], "after");
    Ok(())
}

#[tokio::test]
async fn the_feed_window_is_inclusive_at_both_ends() -> Result<()> {
    let core = core().await;
    let start = day(2031, 1, 1);
    let author = SecretKey::generate();
    let tenant = create_tenant(&core).await?;
    let relay = create_relay(&core, &tenant, "free").await?;

    for offset in 0..3 {
        travel(&core, start + offset);
        save(
            &core,
            &relay.id,
            &[sign_event(&author, 1, &offset.to_string(), &[], start)],
        )
        .await?;
    }

    let page = caravel_operations::moderation::queries::list_moderation(
        core.db.pool(),
        &core.config.admin_pubkeys,
        Some(start + 1),
        Some(start + 2),
    )
    .await?;
    let seen: Vec<i64> = page.items.iter().map(|item| item.seen_at).collect();
    assert_eq!(seen, vec![start + 2, start + 1]);
    Ok(())
}

#[tokio::test]
async fn a_full_page_says_it_was_truncated() -> Result<()> {
    let core = core().await;
    let start = day(2032, 1, 1);
    let author = SecretKey::generate();
    let tenant = create_tenant(&core).await?;
    let relay = create_relay(&core, &tenant, "free").await?;

    travel(&core, start);
    let notes: Vec<Event> = (0..1001)
        .map(|i| sign_event(&author, 1, &i.to_string(), &[], start))
        .collect();
    save(&core, &relay.id, &notes).await?;

    let page = caravel_operations::moderation::queries::list_moderation(
        core.db.pool(),
        &core.config.admin_pubkeys,
        Some(start),
        Some(start),
    )
    .await?;
    assert_eq!(page.items.len(), 1000);
    assert!(page.truncated);
    Ok(())
}

#[tokio::test]
async fn the_feed_leaves_out_what_runs_the_relay() -> Result<()> {
    let core = core().await;
    let start = day(2033, 1, 1);
    let author = SecretKey::generate();
    let tenant = create_tenant(&core).await?;
    let relay = create_relay(&core, &tenant, "free").await?;
    let key = core.relay_key(&relay)?;

    travel(&core, start);
    let note = sign_event(&author, 1, "note", &[], start);
    let room_tag = vec!["d".to_string(), "lobby".to_string()];
    save(
        &core,
        &relay.id,
        &[
            note.clone(),
            key.sign(kinds::rooms::METADATA, vec![room_tag], "", start),
            key.sign(kinds::RELAY_ROLE, vec![], "", start),
            sign_event(
                &author,
                kinds::rooms::JOIN_REQUEST,
                "",
                &[&["h", "lobby"]],
                start,
            ),
            sign_event(&author, kinds::RELAY_LIST, "", &[], start),
            sign_event(&author, kinds::ROOM_LIST, "", &[], start),
        ],
    )
    .await?;

    let page = caravel_operations::moderation::queries::list_moderation(
        core.db.pool(),
        &core.config.admin_pubkeys,
        Some(start),
        Some(start),
    )
    .await?;
    let ids: Vec<String> = page
        .items
        .iter()
        .filter_map(|item| match &item.subject {
            Subject::Event(event) => event["id"].as_str().map(String::from),
            Subject::Upload(_) => None,
        })
        .collect();
    assert_eq!(ids, vec![note.id.to_hex()]);
    Ok(())
}
