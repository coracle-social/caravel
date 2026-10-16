//! The moderation feed, ported from zooid's API tests: every moderated relay's
//! events and uploads, newest first. The feed reads every relay, so each test
//! reads its own window of time.

mod common;

use anyhow::Result;
use caravel_core::events::kinds;
use caravel_core::media;
use caravel_core::moderation::{self, Subject};
use caravel_core::testing::{self, sign_event};
use caravel_core::{Core, events};
use common::*;
use coracle_lib::events::Event;
use coracle_lib::keys::SecretKey;

async fn save(core: &Core, relay_id: &str, events: &[Event]) -> Result<()> {
    let now = core.clock.now();
    core.db
        .with_tx(async |conn| {
            for event in events {
                events::save_event(&mut *conn, relay_id, event, now).await?;
            }
            Ok::<_, caravel_core::Error>(())
        })
        .await?;
    Ok(())
}

async fn upload(core: &Core, relay_id: &str, sha256: &str, pubkey: &str) -> Result<()> {
    let now = core.clock.now();
    core.db
        .with_tx(async |conn| {
            media::record_upload(conn, relay_id, sha256, "image/png", 10, pubkey, now).await
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

    let page = moderation::list_moderation(&core, Some(start), Some(start + 10)).await?;
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

    let page = moderation::list_moderation(&core, Some(start + 1), Some(start + 2)).await?;
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

    let page = moderation::list_moderation(&core, Some(start), Some(start)).await?;
    assert_eq!(page.items.len(), 1000);
    assert!(page.truncated);
    Ok(())
}
