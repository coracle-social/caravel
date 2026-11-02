//! Media and calls: blobs owned by their uploaders and counted once per relay,
//! the call clock, and erasing what a relay holds, purged or not.

use anyhow::Result;
use bytes::Bytes;
use caravel_billing::relays;
use caravel_core::testing::{create_relay, sign_event};
use caravel_core::{Core, calls, hosting, management, media, testing};
use caravel_relays::access::Role;
use caravel_relays::media::{Blob, Deleted};
use caravel_util::bus::Change;
use caravel_util::nostr::kinds;
use coracle_lib::filters::Filter;
use coracle_lib::keys::SecretKey;

const NOW: i64 = 1_800_000_000;

async fn core() -> Core {
    let core = testing::core(&SecretKey::generate().public_key().to_hex()).await;
    core.clock.pin(NOW);
    core
}

fn blob(sha256: &str, size: i64) -> Blob {
    Blob {
        sha256: sha256.to_string(),
        content_type: "image/png".to_string(),
        size,
        created_at: 0,
    }
}

async fn upload(core: &Core, relay_id: &str, blob: &Blob, pubkey: &str) -> Result<Blob> {
    let now = core.clock.now();
    Ok(core
        .db
        .with_tx(async |conn| {
            caravel_relays::media::commands::record_upload(
                conn,
                relay_id,
                &blob.sha256,
                &blob.content_type,
                blob.size,
                pubkey,
                now,
            )
            .await
        })
        .await?)
}

async fn delete(core: &Core, relay_id: &str, sha256: &str, pubkey: &str) -> Result<Deleted> {
    Ok(core
        .db
        .with_tx(async |conn| {
            caravel_relays::media::commands::delete_upload(conn, relay_id, sha256, pubkey).await
        })
        .await?)
}

// Ported from zooid's storage test: a blob counts once however many own it.
#[tokio::test]
async fn storage_counts_each_blob_once() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let (other, _) = create_relay(&core).await;

    upload(&core, &relay.id, &blob("a", 100), "alice").await?;
    upload(&core, &relay.id, &blob("a", 100), "bob").await?;
    upload(&core, &relay.id, &blob("b", 50), "alice").await?;
    upload(&core, &other.id, &blob("a", 100), "carol").await?;

    assert_eq!(
        caravel_relays::media::queries::storage_bytes(core.db.pool(), &relay.id).await?,
        150
    );
    assert_eq!(
        caravel_relays::media::queries::storage_bytes(core.db.pool(), &other.id).await?,
        100
    );
    Ok(())
}

#[tokio::test]
async fn a_blob_goes_with_its_last_owner() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;

    upload(&core, &relay.id, &blob("a", 100), "alice").await?;
    upload(&core, &relay.id, &blob("a", 100), "bob").await?;

    assert_eq!(
        delete(&core, &relay.id, "a", "carol").await?,
        Deleted::NotOwned
    );
    assert_eq!(
        delete(&core, &relay.id, "a", "alice").await?,
        Deleted::Shared
    );
    assert!(
        caravel_relays::media::queries::get_blob(core.db.pool(), &relay.id, "a")
            .await?
            .is_some()
    );
    assert_eq!(delete(&core, &relay.id, "a", "bob").await?, Deleted::Last);
    assert!(
        caravel_relays::media::queries::get_blob(core.db.pool(), &relay.id, "a")
            .await?
            .is_none()
    );
    Ok(())
}

// An upload racing the last owner's deletion of the same blob keeps its bytes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_upload_racing_the_last_owners_deletion_keeps_its_bytes() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let bytes = Bytes::from_static(b"bytes");

    for round in 0..20 {
        let sha256 = format!("race-{round}");
        media::keep_upload(
            &core,
            &relay.id,
            &sha256,
            "image/png",
            "alice",
            bytes.clone(),
        )
        .await?;

        let (kept, deleted) = tokio::join!(
            media::keep_upload(&core, &relay.id, &sha256, "image/png", "bob", bytes.clone()),
            media::delete_upload(&core, &relay.id, &sha256, "alice"),
        );
        kept?;
        assert_ne!(deleted?, Deleted::NotOwned);

        let owned =
            caravel_relays::media::queries::list_blobs_for_pubkey(core.db.pool(), &relay.id, "bob")
                .await?;
        assert!(owned.iter().any(|blob| blob.sha256 == sha256), "{sha256}");
        let key = caravel_relays::media::blob_key(&relay.id, &sha256);
        assert!(core.blobs.size(&key).await?.is_some(), "{sha256}");
    }
    Ok(())
}

#[tokio::test]
async fn a_list_holds_one_uploaders_blobs_newest_first() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;

    upload(&core, &relay.id, &blob("old", 1), "alice").await?;
    core.clock.pin(NOW + 10);
    upload(&core, &relay.id, &blob("new", 2), "alice").await?;
    upload(&core, &relay.id, &blob("theirs", 3), "bob").await?;

    let listed: Vec<String> =
        caravel_relays::media::queries::list_blobs_for_pubkey(core.db.pool(), &relay.id, "alice")
            .await?
            .into_iter()
            .map(|blob| blob.sha256)
            .collect();
    assert_eq!(listed, vec!["new", "old"]);
    Ok(())
}

// Ported from zooid's participant seconds test: the clock sums closed sessions,
// and a redelivered webhook counts once.
#[tokio::test]
async fn the_call_clock_sums_each_session_once() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let (other, _) = create_relay(&core).await;

    assert_eq!(
        caravel_relays::calls::queries::participant_seconds(core.db.pool(), &relay.id).await?,
        0
    );
    caravel_relays::calls::commands::record_session(
        core.db.pool(),
        &relay.id,
        "PA_1",
        "room",
        60,
        NOW,
    )
    .await?;
    caravel_relays::calls::commands::record_session(
        core.db.pool(),
        &relay.id,
        "PA_1",
        "room",
        60,
        NOW,
    )
    .await?;
    caravel_relays::calls::commands::record_session(
        core.db.pool(),
        &relay.id,
        "PA_2",
        "room",
        30,
        NOW,
    )
    .await?;
    caravel_relays::calls::commands::record_session(
        core.db.pool(),
        &other.id,
        "PA_3",
        "room",
        1000,
        NOW,
    )
    .await?;

    assert_eq!(
        caravel_relays::calls::queries::participant_seconds(core.db.pool(), &relay.id).await?,
        90
    );
    Ok(())
}

#[tokio::test]
async fn livekit_rooms_lead_with_their_relay() {
    let name = caravel_relays::calls::livekit_room("relay1", "room:with:colons");
    assert_eq!(
        caravel_relays::calls::split_livekit_room(&name),
        Some(("relay1", "room:with:colons"))
    );
}

#[test]
fn participants_are_named_once_by_pubkey() {
    let pubkey = "a".repeat(64);
    let identities = vec![
        calls::participant_identity(&pubkey),
        calls::participant_identity(&pubkey),
        "garbage".to_string(),
    ];
    assert_eq!(
        caravel_relays::calls::participant_pubkeys(&identities),
        vec![pubkey]
    );
}

#[tokio::test]
async fn erasing_a_purged_relay_removes_what_it_held() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let (kept, _) = create_relay(&core).await;
    let author = SecretKey::generate();

    for relay_id in [&relay.id, &kept.id] {
        let event = sign_event(&author, 1, "hello", &[], NOW);
        core.db
            .with_tx(async |conn| {
                caravel_relays::events::commands::save_event(conn, relay_id, &event, NOW).await
            })
            .await?;
        upload(&core, relay_id, &blob("a", 100), "alice").await?;
        core.blobs
            .put(
                &caravel_relays::media::blob_key(relay_id, "a"),
                Bytes::from_static(b"bytes"),
            )
            .await?;
    }

    hosting::purge_relay(&core, &relay).await?;
    hosting::erase_purged_relays(&core).await?;

    let filter = Filter::new();
    assert!(
        caravel_relays::events::queries::query_events(core.db.pool(), &relay.id, &filter, 10, NOW)
            .await?
            .is_empty()
    );
    assert!(
        caravel_relays::media::queries::get_blob(core.db.pool(), &relay.id, "a")
            .await?
            .is_none()
    );
    assert!(
        core.blobs
            .get(&caravel_relays::media::blob_key(&relay.id, "a"), None)
            .await?
            .is_none()
    );
    let erased = caravel_billing::relays::queries::get_relay(core.db.pool(), &relay.id)
        .await?
        .unwrap();
    assert_eq!(erased.erased_at, Some(NOW));

    assert!(
        !caravel_relays::events::queries::query_events(core.db.pool(), &kept.id, &filter, 10, NOW)
            .await?
            .is_empty()
    );
    assert!(
        core.blobs
            .get(&caravel_relays::media::blob_key(&kept.id, "a"), None)
            .await?
            .is_some()
    );

    // Erasure runs once per relay.
    hosting::erase_purged_relays(&core).await?;
    Ok(())
}

#[tokio::test]
async fn erasing_a_relay_keeps_only_its_settings_and_owner() -> Result<()> {
    let core = core().await;
    let (relay, owner) = create_relay(&core).await;
    let (kept, _) = create_relay(&core).await;
    let key = core.relay_key(&relay)?;
    let member = SecretKey::generate().public_key().to_hex();
    let banned = SecretKey::generate().public_key().to_hex();

    management::add_member(&core, &relay, &member).await?;
    let note = sign_event(&owner, 1, "hello", &[], NOW);
    let room = sign_event(
        &owner,
        kinds::rooms::CREATE_GROUP,
        "",
        &[&["h", "lobby"]],
        NOW,
    );
    let role = Role {
        id: "mod".to_string(),
        ..Default::default()
    };
    core.db
        .with_tx(async |conn| {
            caravel_relays::events::commands::save_event(&mut *conn, &relay.id, &note, NOW).await?;
            caravel_relays::rooms::commands::apply_event(&mut *conn, &key, &relay.id, &room, NOW)
                .await?;
            caravel_relays::access::commands::create_role(&mut *conn, &key, &relay.id, &role, NOW)
                .await?;
            caravel_relays::access::commands::ban_pubkey(
                &mut *conn,
                &key,
                &relay.id,
                &relay.tenant_pubkey,
                &banned,
                "spam",
                NOW,
            )
            .await?;
            caravel_relays::access::commands::create_claim(&mut *conn, &relay.id, "sesame", NOW)
                .await
        })
        .await?;
    for relay_id in [&relay.id, &kept.id] {
        upload(&core, relay_id, &blob("a", 100), &member).await?;
        core.blobs
            .put(
                &caravel_relays::media::blob_key(relay_id, "a"),
                Bytes::from_static(b"bytes"),
            )
            .await?;
    }

    hosting::erase_relay(&core, &relay).await?;

    // Only the member list the relay signed again is left.
    let pool = core.db.pool();
    let left =
        caravel_relays::events::queries::query_events(pool, &relay.id, &Filter::new(), 10, NOW)
            .await?;
    assert_eq!(
        left.iter().map(|event| event.head.kind).collect::<Vec<_>>(),
        [kinds::RELAY_MEMBERS]
    );
    assert_eq!(
        caravel_relays::access::queries::list_members(pool, &relay.id)
            .await?
            .into_iter()
            .map(|member| member.pubkey)
            .collect::<Vec<_>>(),
        std::slice::from_ref(&relay.tenant_pubkey)
    );
    assert!(
        caravel_relays::rooms::queries::list_rooms(pool, &relay.id)
            .await?
            .is_empty()
    );
    assert!(
        caravel_relays::access::queries::list_roles(pool, &relay.id)
            .await?
            .is_empty()
    );
    assert!(
        caravel_relays::access::queries::list_banned_pubkeys(pool, &relay.id)
            .await?
            .is_empty()
    );
    assert!(
        caravel_relays::access::queries::list_claims(pool, &relay.id)
            .await?
            .is_empty()
    );
    assert!(
        caravel_relays::media::queries::get_blob(pool, &relay.id, "a")
            .await?
            .is_none()
    );
    assert!(
        core.blobs
            .get(&caravel_relays::media::blob_key(&relay.id, "a"), None)
            .await?
            .is_none()
    );

    let erased = caravel_billing::relays::queries::require_relay(pool, &relay.id).await?;
    assert_eq!(
        serde_json::to_value(&erased)?,
        serde_json::to_value(&relay)?
    );

    assert!(
        caravel_relays::media::queries::get_blob(pool, &kept.id, "a")
            .await?
            .is_some()
    );
    assert!(
        core.blobs
            .get(&caravel_relays::media::blob_key(&kept.id, "a"), None)
            .await?
            .is_some()
    );
    Ok(())
}

// The erase has committed by the time bytes are deleted, so a failure there is
// left for the purge and the relay still reloads.
#[tokio::test]
async fn erasing_a_relay_outlasts_bytes_that_wont_delete() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("caravel-erase-{}", uuid::Uuid::new_v4()));
    let mut core = core().await;
    core.blobs = caravel_util::blobs::testing::local(dir.to_str().expect("a utf-8 temp dir"))?;
    let (relay, _) = create_relay(&core).await;

    for sha256 in ["a", "b"] {
        upload(&core, &relay.id, &blob(sha256, 100), &relay.tenant_pubkey).await?;
    }
    core.blobs
        .put(
            &caravel_relays::media::blob_key(&relay.id, "b"),
            Bytes::from_static(b"bytes"),
        )
        .await?;
    // A directory where a's bytes belong can't be deleted as a file.
    std::fs::create_dir_all(dir.join(caravel_relays::media::blob_key(&relay.id, "a")))?;

    let mut changes = core.bus.subscribe();
    hosting::erase_relay(&core, &relay).await?;

    assert!(matches!(
        changes.try_recv(),
        Ok(Change::AccessChanged { relay_id, .. }) if relay_id == relay.id
    ));
    assert!(
        core.blobs
            .get(&caravel_relays::media::blob_key(&relay.id, "b"), None)
            .await?
            .is_none()
    );

    std::fs::remove_dir_all(dir)?;
    Ok(())
}

// Deleting a relay leaves its subdomain for another.
#[tokio::test]
async fn a_purged_relays_subdomain_can_be_taken_again() -> Result<()> {
    let core = core().await;
    let (relay, owner) = create_relay(&core).await;
    let settings = || relays::RelaySettings {
        subdomain: Some(relay.subdomain.clone()),
        domain: Some(relay.domain.clone()),
        plan_id: Some("free".to_string()),
        ..Default::default()
    };
    let owner = owner.public_key().to_hex();

    let taken = hosting::create_relay(&core, &owner, settings()).await;
    assert!(matches!(
        taken,
        Err(caravel_util::error::Error::Invalid {
            code: "subdomain-exists",
            ..
        })
    ));

    hosting::purge_relay(&core, &relay).await?;
    let again = hosting::create_relay(&core, &owner, settings()).await?;
    assert_eq!(again.subdomain, relay.subdomain);
    assert_ne!(again.id, relay.id);
    Ok(())
}
