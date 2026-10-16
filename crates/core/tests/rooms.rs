//! Rooms, ported from zooid's group tests: how events name their room, what each
//! moderation event does to a room's state, who may publish what into a room, and
//! that the events the relay publishes about its state can be rebuilt from it.

use std::collections::BTreeMap;

use anyhow::Result;
use caravel_core::Core;
use caravel_core::crypto::RelayKey;
use caravel_core::events::{self, Saved};
use caravel_core::management::{self, Call};
use caravel_core::relay_policy::{self, Refusal, RelaySnapshot};
use caravel_core::relays::Relay;
use caravel_core::rooms::{self, kinds};
use caravel_core::testing::{self, create_relay, sign_event};
use coracle_lib::events::Event;
use coracle_lib::filters::Filter;
use coracle_lib::keys::SecretKey;

struct Fixture {
    core: Core,
    relay: Relay,
    key: RelayKey,
    owner: SecretKey,
}

async fn fixture() -> Fixture {
    let core = testing::core(&SecretKey::generate().public_key().to_hex()).await;
    let (relay, owner) = create_relay(&core).await;
    let key = core.relay_key(&relay).unwrap();
    Fixture {
        core,
        relay,
        key,
        owner,
    }
}

impl Fixture {
    fn now(&self) -> i64 {
        self.core.clock.now()
    }

    async fn snapshot(&self) -> RelaySnapshot {
        relay_policy::load_snapshot(self.core.db.pool(), &self.relay)
            .await
            .unwrap()
    }

    /// Publish an event the way the relay ingests one: check it against the room
    /// rules, store it, and apply it to its room.
    async fn publish(
        &self,
        author: &SecretKey,
        kind: u16,
        tags: &[&[&str]],
    ) -> Result<Vec<Event>, Refusal> {
        let event = sign_event(author, kind, "", tags, self.now());
        relay_policy::check_room_write(&self.snapshot().await, &event)?;

        let now = self.now();
        let published = self
            .core
            .db
            .with_tx(async |conn| {
                let saved = events::save_event(&mut *conn, &self.relay.id, &event, now).await?;
                assert_eq!(saved, Saved::New);
                rooms::apply_event(conn, &self.key, &self.relay.id, &event, now).await
            })
            .await
            .unwrap();
        Ok(published)
    }

    async fn store(&self, event: &Event) {
        let now = self.now();
        self.core
            .db
            .with_tx(async |conn| events::save_event(conn, &self.relay.id, event, now).await)
            .await
            .unwrap();
    }

    async fn exists(&self, event: &Event) -> bool {
        !events::get_events(self.core.db.pool(), &self.relay.id, &[event.id.to_bytes()])
            .await
            .unwrap()
            .is_empty()
    }

    /// The relay's published events of a kind about a room.
    async fn published(&self, kind: u16, room: &str) -> Option<Event> {
        let filter =
            Filter::new()
                .add_kind(kind)
                .add_tag(coracle_lib::filters::TagMatch::Any, "d", room);
        let found =
            events::query_events(self.core.db.pool(), &self.relay.id, &filter, 1, self.now())
                .await
                .unwrap();
        found
            .first()
            .map(|stored| serde_json::from_str(&stored.to_json(false)).unwrap())
    }
}

fn values(event: &Event, name: &str) -> Vec<String> {
    event.tags.values(name).map(str::to_string).collect()
}

#[test]
fn an_event_names_its_room_by_d_for_room_state_and_h_otherwise() {
    let author = SecretKey::generate();
    let message = sign_event(&author, 9, "hi", &[&["h", "room"]], 0);
    assert_eq!(rooms::room_id(&message), Some("room"));

    let pins = sign_event(&author, kinds::PINS, "", &[&["d", "room"]], 0);
    assert_eq!(rooms::room_id(&pins), Some("room"));

    let put_pins = sign_event(&author, kinds::PUT_PINS, "", &[&["h", "room"]], 0);
    assert_eq!(rooms::room_id(&put_pins), Some("room"));

    let elsewhere = sign_event(&author, 1, "hi", &[&["p", "x"]], 0);
    assert_eq!(rooms::room_id(&elsewhere), None);
}

#[tokio::test]
async fn creating_a_room_publishes_its_metadata_and_admins() -> Result<()> {
    let f = fixture().await;
    let published = f
        .publish(
            &f.owner,
            kinds::CREATE_GROUP,
            &[&["h", "lobby"], &["name", "Lobby"], &["private"]],
        )
        .await
        .unwrap();

    assert_eq!(
        published.iter().map(|event| event.kind).collect::<Vec<_>>(),
        [39000, 39001]
    );
    let metadata = f.published(kinds::METADATA, "lobby").await.unwrap();
    assert_eq!(metadata.tags.value("name"), Some("Lobby"));
    assert!(metadata.tags.has("private"));
    assert!(!metadata.tags.has("h"));

    let admins = f.published(kinds::ADMINS, "lobby").await.unwrap();
    assert_eq!(values(&admins, "p"), [f.owner.public_key().to_hex()]);

    let again = f
        .publish(&f.owner, kinds::CREATE_GROUP, &[&["h", "lobby"]])
        .await;
    assert_eq!(again, Err(Refusal::invalid("that group already exists")));
    Ok(())
}

#[tokio::test]
async fn only_moderators_change_a_room_and_nobody_writes_its_state_directly() -> Result<()> {
    let f = fixture().await;
    let member = SecretKey::generate();
    f.publish(&f.owner, kinds::CREATE_GROUP, &[&["h", "room"]])
        .await
        .unwrap();

    let pinned = "a".repeat(64);
    f.publish(
        &f.owner,
        kinds::PUT_PINS,
        &[&["h", "room"], &["e", &pinned]],
    )
    .await
    .unwrap();
    let pins = f.published(kinds::PINS, "room").await.unwrap();
    assert_eq!(values(&pins, "e"), [pinned]);

    assert_eq!(
        f.publish(&member, kinds::PUT_PINS, &[&["h", "room"]]).await,
        Err(Refusal::restricted(
            "you are not authorized to manage groups"
        ))
    );
    assert_eq!(
        f.publish(&f.owner, kinds::PINS, &[&["d", "room"]]).await,
        Err(Refusal::invalid("group metadata cannot be set directly"))
    );
    assert_eq!(
        f.publish(&f.owner, kinds::PUT_PINS, &[&["h", "nowhere"]])
            .await,
        Err(Refusal::invalid("group not found"))
    );
    Ok(())
}

#[tokio::test]
async fn a_relay_method_holder_may_not_moderate_rooms_but_a_room_admin_may() -> Result<()> {
    let f = fixture().await;
    let holder = SecretKey::generate();
    let admin = SecretKey::generate();
    management::call(
        &f.core,
        &f.relay,
        Call::AssignMethod(holder.public_key().to_hex(), "banevent".into()),
    )
    .await?;
    f.publish(&f.owner, kinds::CREATE_GROUP, &[&["h", "room"]])
        .await
        .unwrap();
    f.publish(
        &f.owner,
        kinds::PUT_USER,
        &[
            &["h", "room"],
            &["p", &admin.public_key().to_hex(), "admin"],
        ],
    )
    .await
    .unwrap();

    let target = "b".repeat(64);
    assert!(
        f.publish(
            &holder,
            kinds::DELETE_EVENT,
            &[&["h", "room"], &["e", &target]]
        )
        .await
        .is_err()
    );
    assert!(
        f.publish(
            &admin,
            kinds::DELETE_EVENT,
            &[&["h", "room"], &["e", &target]]
        )
        .await
        .is_ok()
    );

    let admins = f.published(kinds::ADMINS, "room").await.unwrap();
    let admin_tag: Vec<String> = admins
        .tags
        .find_all("p")
        .find(|tag| tag.value() == admin.public_key().to_hex())
        .unwrap()
        .as_slice()
        .to_vec();
    assert_eq!(admin_tag[2], "admin");
    Ok(())
}

#[tokio::test]
async fn joining_and_leaving_a_room_is_published_by_the_relay() -> Result<()> {
    let f = fixture().await;
    let alice = SecretKey::generate();
    let alice_hex = alice.public_key().to_hex();
    f.publish(&f.owner, kinds::CREATE_GROUP, &[&["h", "room"]])
        .await
        .unwrap();

    let joined = f
        .publish(&alice, kinds::JOIN_REQUEST, &[&["h", "room"]])
        .await
        .unwrap();
    assert_eq!(joined[0].kind, kinds::PUT_USER);
    assert_eq!(joined[0].pubkey.to_hex(), f.key.pubkey());
    assert_eq!(
        values(&f.published(kinds::MEMBERS, "room").await.unwrap(), "p"),
        std::slice::from_ref(&alice_hex)
    );

    assert_eq!(
        f.publish(&alice, kinds::JOIN_REQUEST, &[&["h", "room"]])
            .await,
        Err(Refusal::Unchanged("already a member".into()))
    );

    let left = f
        .publish(&alice, kinds::LEAVE_REQUEST, &[&["h", "room"]])
        .await
        .unwrap();
    assert_eq!(left[0].kind, kinds::REMOVE_USER);
    assert!(values(&f.published(kinds::MEMBERS, "room").await.unwrap(), "p").is_empty());

    assert_eq!(
        f.publish(&alice, kinds::LEAVE_REQUEST, &[&["h", "room"]])
            .await,
        Err(Refusal::Unchanged("not currently a member".into()))
    );
    Ok(())
}

#[tokio::test]
async fn a_closed_room_takes_joins_only_with_an_invite_and_a_hidden_one_hides() -> Result<()> {
    let f = fixture().await;
    let outsider = SecretKey::generate();
    f.publish(
        &f.owner,
        kinds::CREATE_GROUP,
        &[&["h", "vault"], &["closed"], &["hidden"]],
    )
    .await
    .unwrap();
    f.publish(
        &f.owner,
        kinds::CREATE_INVITE,
        &[&["h", "vault"], &["code", "letmein"]],
    )
    .await
    .unwrap();

    assert_eq!(
        f.publish(&outsider, kinds::JOIN_REQUEST, &[&["h", "vault"]])
            .await,
        Err(Refusal::invalid("group not found"))
    );
    assert_eq!(
        f.publish(&outsider, 9, &[&["h", "vault"]]).await,
        Err(Refusal::invalid("group not found"))
    );

    let joined = f
        .publish(
            &outsider,
            kinds::JOIN_REQUEST,
            &[&["h", "vault"], &["code", "letmein"]],
        )
        .await
        .unwrap();
    assert_eq!(joined[0].kind, kinds::PUT_USER);
    assert!(f.publish(&outsider, 9, &[&["h", "vault"]]).await.is_ok());
    Ok(())
}

#[tokio::test]
async fn deleting_events_stays_within_the_room_and_spares_its_state() -> Result<()> {
    let f = fixture().await;
    let author = SecretKey::generate();
    f.publish(&f.owner, kinds::CREATE_GROUP, &[&["h", "room"]])
        .await
        .unwrap();
    f.publish(&f.owner, kinds::CREATE_GROUP, &[&["h", "other"]])
        .await
        .unwrap();

    let here = sign_event(&author, 9, "here", &[&["h", "room"]], f.now());
    let there = sign_event(&author, 9, "there", &[&["h", "other"]], f.now());
    f.store(&here).await;
    f.store(&there).await;
    let metadata = f.published(kinds::METADATA, "room").await.unwrap();

    let ids = [here.id.to_hex(), there.id.to_hex(), metadata.id.to_hex()];
    f.publish(
        &f.owner,
        kinds::DELETE_EVENT,
        &[
            &["h", "room"],
            &["e", &ids[0]],
            &["e", &ids[1]],
            &["e", &ids[2]],
        ],
    )
    .await
    .unwrap();

    assert!(!f.exists(&here).await);
    assert!(f.exists(&there).await, "another room's event stays");
    assert!(f.exists(&metadata).await, "the room's own state stays");
    Ok(())
}

#[tokio::test]
async fn deleting_a_room_removes_it_and_everything_in_it() -> Result<()> {
    let f = fixture().await;
    f.publish(&f.owner, kinds::CREATE_GROUP, &[&["h", "room"]])
        .await
        .unwrap();
    f.publish(
        &f.owner,
        kinds::PUT_PINS,
        &[&["h", "room"], &["e", &"c".repeat(64)]],
    )
    .await
    .unwrap();
    let message = sign_event(&f.owner, 9, "bye", &[&["h", "room"]], f.now());
    f.store(&message).await;

    f.publish(&f.owner, kinds::DELETE_GROUP, &[&["h", "room"]])
        .await
        .unwrap();

    assert!(f.snapshot().await.rooms.is_empty());
    assert!(f.published(kinds::PINS, "room").await.is_none());
    assert!(f.published(kinds::METADATA, "room").await.is_none());
    assert!(!f.exists(&message).await);
    Ok(())
}

#[tokio::test]
async fn rooms_that_are_off_take_no_room_events() -> Result<()> {
    let f = fixture().await;
    let mut snapshot = f.snapshot().await;
    snapshot.groups_enabled = false;
    let event = sign_event(
        &f.owner,
        kinds::CREATE_GROUP,
        "",
        &[&["h", "room"]],
        f.now(),
    );
    assert_eq!(
        relay_policy::check_room_write(&snapshot, &event),
        Err(Refusal::invalid("groups are not enabled"))
    );
    Ok(())
}

/// The relay's published state is derived from its tables, so rewriting it all from
/// them changes nothing.
#[tokio::test]
async fn rebuilding_the_published_state_reproduces_it() -> Result<()> {
    let f = fixture().await;
    let alice = SecretKey::generate();
    let bob = SecretKey::generate();

    management::call(
        &f.core,
        &f.relay,
        Call::CreateRole(caravel_core::access::Role {
            id: "mod".into(),
            label: "Mod".into(),
            ..Default::default()
        }),
    )
    .await?;
    management::call(
        &f.core,
        &f.relay,
        Call::AssignRole(alice.public_key().to_hex(), "mod".into()),
    )
    .await?;
    management::call(
        &f.core,
        &f.relay,
        Call::AssignMethod(bob.public_key().to_hex(), "banevent".into()),
    )
    .await?;
    f.publish(
        &f.owner,
        kinds::CREATE_GROUP,
        &[&["h", "room"], &["name", "Room"]],
    )
    .await
    .unwrap();
    f.publish(&alice, kinds::JOIN_REQUEST, &[&["h", "room"]])
        .await
        .unwrap();
    f.publish(
        &f.owner,
        kinds::PUT_USER,
        &[&["h", "room"], &["p", &bob.public_key().to_hex(), "admin"]],
    )
    .await
    .unwrap();
    f.publish(
        &f.owner,
        kinds::PUT_PINS,
        &[&["h", "room"], &["e", &"d".repeat(64)]],
    )
    .await
    .unwrap();

    let current = published_state(&f).await;
    let now = f.now();
    let rebuilt = f
        .core
        .db
        .with_tx(async |conn| management::rebuild_derived(conn, &f.key, &f.relay.id, now).await)
        .await?;

    let rebuilt: BTreeMap<(u16, String), Vec<Vec<String>>> = rebuilt
        .iter()
        .map(|event| {
            (
                (
                    event.kind,
                    event.tags.value("d").unwrap_or_default().to_string(),
                ),
                tags(event),
            )
        })
        .collect();
    assert_eq!(current, rebuilt);
    assert_eq!(published_state(&f).await, rebuilt);
    Ok(())
}

async fn published_state(f: &Fixture) -> BTreeMap<(u16, String), Vec<Vec<String>>> {
    let filter = Filter::new()
        .add_author(f.key.pubkey().parse().unwrap())
        .add_kinds([13534, 33534, 39000, 39001, 39002, 39005]);
    events::query_events(f.core.db.pool(), &f.relay.id, &filter, 1000, f.now())
        .await
        .unwrap()
        .iter()
        .map(|stored| {
            let event: Event = serde_json::from_str(&stored.to_json(false)).unwrap();
            (
                (
                    event.kind,
                    event.tags.value("d").unwrap_or_default().to_string(),
                ),
                tags(&event),
            )
        })
        .collect()
}

fn tags(event: &Event) -> Vec<Vec<String>> {
    event
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .collect()
}
