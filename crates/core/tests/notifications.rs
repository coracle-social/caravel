//! Notifications, ported from zooid's notify tests: one message per pubkey per
//! relay, replaced, cleared or expired.

use anyhow::Result;
use caravel_core::bus::Change;
use caravel_core::notifications::{self, get_notification};
use caravel_core::testing::{self, create_relay};
use caravel_core::{Core, Error};
use coracle_lib::keys::SecretKey;

const NOW: i64 = 1_800_000_000;

async fn core() -> Core {
    let core = testing::core(&SecretKey::generate().public_key().to_hex()).await;
    core.clock.pin(NOW);
    core
}

fn pubkey() -> String {
    SecretKey::generate().public_key().to_hex()
}

#[tokio::test]
async fn a_message_is_kept_and_announced() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let member = pubkey();
    let mut changes = core.bus.subscribe();

    notifications::notify(&core, &relay.id, &member, "your membership lapsed", None).await?;

    let kept = get_notification(core.db.pool(), &relay.id, &member, NOW)
        .await?
        .unwrap();
    assert_eq!(kept.message, "your membership lapsed");
    assert_eq!(kept.created_at, NOW);
    assert_eq!(
        changes.recv().await?,
        Change::Notified {
            relay_id: relay.id.clone(),
            pubkey: member,
            message: "your membership lapsed".to_string(),
        }
    );
    Ok(())
}

#[tokio::test]
async fn a_new_message_replaces_the_old() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let member = pubkey();

    notifications::notify(&core, &relay.id, &member, "first", Some(NOW + 60)).await?;
    notifications::notify(&core, &relay.id, &member, "second", None).await?;

    let kept = get_notification(core.db.pool(), &relay.id, &member, NOW)
        .await?
        .unwrap();
    assert_eq!((kept.message.as_str(), kept.expires_at), ("second", None));
    Ok(())
}

#[tokio::test]
async fn a_message_is_required() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;

    let result = notifications::notify(&core, &relay.id, &pubkey(), "", None).await;
    assert!(matches!(result, Err(Error::Invalid { .. })));
    Ok(())
}

#[tokio::test]
async fn messages_belong_to_one_pubkey_on_one_relay() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let (other_relay, _) = create_relay(&core).await;
    let member = pubkey();

    notifications::notify(&core, &relay.id, &member, "hello", None).await?;

    assert!(
        get_notification(core.db.pool(), &relay.id, &pubkey(), NOW)
            .await?
            .is_none()
    );
    assert!(
        get_notification(core.db.pool(), &other_relay.id, &member, NOW)
            .await?
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn a_message_stops_at_its_expiry() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let member = pubkey();

    notifications::notify(&core, &relay.id, &member, "brief", Some(NOW + 60)).await?;

    assert!(
        get_notification(core.db.pool(), &relay.id, &member, NOW + 59)
            .await?
            .is_some()
    );
    assert!(
        get_notification(core.db.pool(), &relay.id, &member, NOW + 60)
            .await?
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn clearing_a_message_is_idempotent() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let member = pubkey();

    notifications::notify(&core, &relay.id, &member, "hello", None).await?;
    for _ in 0..2 {
        notifications::clear(core.db.pool(), &relay.id, &member).await?;
        assert!(
            get_notification(core.db.pool(), &relay.id, &member, NOW)
                .await?
                .is_none()
        );
    }
    Ok(())
}
