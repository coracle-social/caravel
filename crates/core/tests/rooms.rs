//! Rooms, ported from zooid's group tests: how events name their room, what each
//! moderation event does to a room's state, who may publish what into a room, and
//! that the events the relay publishes about its state can be rebuilt from it.

use std::collections::BTreeMap;

use anyhow::Result;
use caravel_billing::relays::Relay;
use caravel_core::testing::{create_relay, sign_event};
use caravel_core::{Core, management, testing};
use caravel_relays::events::Saved;
use caravel_relays::management::Call;
use caravel_relays::policy::{Refusal, RelaySnapshot};
use caravel_relays::rooms;
use caravel_util::nostr::Key;
use caravel_util::nostr::kinds::rooms as kinds;
use coracle_lib::events::Event;
use coracle_lib::filters::Filter;
use coracle_lib::keys::{PublicKey, SecretKey};

struct Fixture {
    core: Core,
    relay: Relay,
    key: Key,
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
        caravel_core::events::load_snapshot(&self.core, &self.relay)
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
        self.snapshot().await.check_room_write(&event)?;

        let now = self.now();
        let published = self
            .core
            .db
            .with_tx(async |conn| {
                let saved = caravel_relays::events::commands::save_event(
                    &mut *conn,
                    &self.relay.id,
                    &event,
                    now,
                )
                .await?;
                assert_eq!(saved, Saved::New);
                caravel_relays::rooms::commands::apply_event(
                    conn,
                    &self.key,
                    &self.relay.id,
                    &event,
                    now,
                )
                .await
            })
            .await
            .unwrap();
        Ok(published)
    }

    async fn store(&self, event: &Event) -> Saved {
        let now = self.now();
        self.core
            .db
            .with_tx(async |conn| {
                caravel_relays::events::commands::save_event(conn, &self.relay.id, event, now).await
            })
            .await
            .unwrap()
    }

    async fn exists(&self, event: &Event) -> bool {
        !caravel_relays::events::queries::get_events(
            self.core.db.pool(),
            &self.relay.id,
            &[event.id.to_bytes()],
        )
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
        let found = caravel_relays::events::queries::query_events(
            self.core.db.pool(),
            &self.relay.id,
            &filter,
            1,
            self.now(),
        )
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
    assert_eq!(caravel_relays::rooms::room_id(&message), Some("room"));

    let pins = sign_event(&author, kinds::PINS, "", &[&["d", "room"]], 0);
    assert_eq!(caravel_relays::rooms::room_id(&pins), Some("room"));

    let put_pins = sign_event(&author, kinds::PUT_PINS, "", &[&["h", "room"]], 0);
    assert_eq!(caravel_relays::rooms::room_id(&put_pins), Some("room"));

    let elsewhere = sign_event(&author, 1, "hi", &[&["p", "x"]], 0);
    assert_eq!(caravel_relays::rooms::room_id(&elsewhere), None);
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
    let address = format!("30023:{}:post", member.public_key().to_hex());
    f.publish(
        &f.owner,
        kinds::PUT_PINS,
        &[&["h", "room"], &["e", &pinned], &["a", &address]],
    )
    .await
    .unwrap();
    let pins = f.published(kinds::PINS, "room").await.unwrap();
    assert_eq!(values(&pins, "e"), [pinned]);
    assert_eq!(values(&pins, "a"), [address]);

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
async fn editing_a_room_replaces_its_metadata() -> Result<()> {
    let f = fixture().await;
    f.publish(
        &f.owner,
        kinds::CREATE_GROUP,
        &[&["h", "room"], &["name", "Room"], &["private"]],
    )
    .await
    .unwrap();

    let published = f
        .publish(
            &f.owner,
            kinds::EDIT_METADATA,
            &[
                &["h", "room"],
                &["name", "Renamed"],
                &["about", "A room"],
                &["closed"],
            ],
        )
        .await
        .unwrap();
    assert_eq!(
        published.iter().map(|event| event.kind).collect::<Vec<_>>(),
        [39000, 39001]
    );
    let metadata = f.published(kinds::METADATA, "room").await.unwrap();
    assert_eq!(metadata.tags.value("name"), Some("Renamed"));
    assert_eq!(metadata.tags.value("about"), Some("A room"));
    assert!(metadata.tags.has("closed"));
    assert!(
        !metadata.tags.has("private"),
        "flags the edit leaves out are dropped"
    );

    let snapshot = f.snapshot().await;
    let room = &snapshot.rooms["room"];
    assert!(room.closed && !room.private);

    assert_eq!(
        f.publish(
            &f.owner,
            kinds::EDIT_METADATA,
            &[&["h", "nowhere"], &["name", "x"]]
        )
        .await,
        Err(Refusal::invalid("group not found"))
    );
    Ok(())
}

#[tokio::test]
async fn moderators_put_and_remove_members() -> Result<()> {
    let f = fixture().await;
    let (alice, bob) = (SecretKey::generate(), SecretKey::generate());
    let (alice, bob) = (alice.public_key().to_hex(), bob.public_key().to_hex());
    f.publish(&f.owner, kinds::CREATE_GROUP, &[&["h", "room"]])
        .await
        .unwrap();

    let published = f
        .publish(
            &f.owner,
            kinds::PUT_USER,
            &[&["h", "room"], &["p", &alice], &["p", &bob]],
        )
        .await
        .unwrap();
    assert!(published.iter().any(|event| event.kind == kinds::MEMBERS));
    let mut members = values(&f.published(kinds::MEMBERS, "room").await.unwrap(), "p");
    members.sort();
    let mut expected = vec![alice.clone(), bob.clone()];
    expected.sort();
    assert_eq!(members, expected);

    let published = f
        .publish(
            &f.owner,
            kinds::REMOVE_USER,
            &[&["h", "room"], &["p", &bob]],
        )
        .await
        .unwrap();
    assert!(published.iter().any(|event| event.kind == kinds::MEMBERS));
    assert_eq!(
        values(&f.published(kinds::MEMBERS, "room").await.unwrap(), "p"),
        std::slice::from_ref(&alice)
    );
    assert!(
        !f.snapshot().await.rooms["room"]
            .members
            .contains(&PublicKey::from_hex(&bob).unwrap())
    );
    Ok(())
}

#[tokio::test]
async fn a_closed_room_admits_only_joins_with_its_invite() -> Result<()> {
    let f = fixture().await;
    let (alice, bob) = (SecretKey::generate(), SecretKey::generate());
    f.publish(
        &f.owner,
        kinds::CREATE_GROUP,
        &[&["h", "club"], &["closed"], &["restricted"]],
    )
    .await
    .unwrap();
    f.publish(
        &f.owner,
        kinds::CREATE_INVITE,
        &[&["h", "club"], &["code", "sesame"]],
    )
    .await
    .unwrap();

    // A request without the code is kept, but admits nobody.
    for tags in [
        &[&["h", "club"][..]][..],
        &[&["h", "club"], &["code", "guess"]],
    ] {
        let published = f.publish(&alice, kinds::JOIN_REQUEST, tags).await.unwrap();
        assert!(published.is_empty());
    }
    assert_eq!(
        f.publish(&alice, 9, &[&["h", "club"]]).await,
        Err(Refusal::restricted("you are not a member of that group"))
    );

    // The code admits everyone who has it.
    for joiner in [&alice, &bob] {
        let published = f
            .publish(
                joiner,
                kinds::JOIN_REQUEST,
                &[&["h", "club"], &["code", "sesame"]],
            )
            .await
            .unwrap();
        assert_eq!(published[0].kind, kinds::PUT_USER);
        assert!(f.publish(joiner, 9, &[&["h", "club"]]).await.is_ok());
    }
    Ok(())
}

#[tokio::test]
async fn a_room_is_moderated_by_the_kinds_its_members_are_permitted() -> Result<()> {
    let f = fixture().await;
    let holder = SecretKey::generate();
    let titled = SecretKey::generate();
    let moderator = SecretKey::generate();
    let moderator_hex = moderator.public_key().to_hex();
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
            &["p", &titled.public_key().to_hex(), "admin"],
        ],
    )
    .await
    .unwrap();
    f.publish(
        &f.owner,
        kinds::ASSIGN_PERMISSION,
        &[&["h", "room"], &["p", &moderator_hex, "9005", "9011"]],
    )
    .await
    .unwrap();

    // Neither a relay method nor a role grants anything in a room.
    let target = "b".repeat(64);
    let delete = [&["h", "room"][..], &["e", &target][..]];
    let refused = Err(Refusal::restricted(
        "you are not authorized to manage groups",
    ));
    assert_eq!(
        f.publish(&holder, kinds::DELETE_EVENT, &delete)
            .await
            .map(drop),
        refused
    );
    assert_eq!(
        f.publish(&titled, kinds::DELETE_EVENT, &delete)
            .await
            .map(drop),
        refused
    );
    assert!(
        f.publish(&moderator, kinds::DELETE_EVENT, &delete)
            .await
            .is_ok()
    );
    assert_eq!(
        f.publish(
            &moderator,
            kinds::REMOVE_USER,
            &[&["h", "room"], &["p", &titled.public_key().to_hex()]]
        )
        .await
        .map(drop),
        refused
    );

    // A moderator may pass on only the kinds they hold.
    let titled_hex = titled.public_key().to_hex();
    assert_eq!(
        f.publish(
            &moderator,
            kinds::ASSIGN_PERMISSION,
            &[&["h", "room"], &["p", &titled_hex, "9008"]]
        )
        .await
        .map(drop),
        Err(Refusal::restricted(
            "you may only grant permissions you hold"
        ))
    );
    assert_eq!(
        f.publish(
            &f.owner,
            kinds::ASSIGN_PERMISSION,
            &[&["h", "room"], &["p", &titled_hex, "1"]]
        )
        .await
        .map(drop),
        Err(Refusal::invalid("permissions are group management kinds"))
    );
    // A grant that names no kind would only put the target in the room.
    assert_eq!(
        f.publish(
            &f.owner,
            kinds::ASSIGN_PERMISSION,
            &[&["h", "room"], &["p", &titled_hex]]
        )
        .await
        .map(drop),
        Err(Refusal::invalid(
            "a permission tag has to name a group management kind"
        ))
    );
    f.publish(
        &moderator,
        kinds::ASSIGN_PERMISSION,
        &[&["h", "room"], &["p", &titled_hex, "9005"]],
    )
    .await
    .unwrap();

    // The admin list names each admin's kinds, and the member list their roles,
    // in the order they joined, which here is the same second.
    let mut admins: Vec<Vec<String>> = f
        .published(kinds::ADMINS, "room")
        .await
        .unwrap()
        .tags
        .find_all("p")
        .map(|tag| tag.as_slice().to_vec())
        .collect();
    admins[1..].sort();
    let owner_hex = f.owner.public_key().to_hex();
    let mut owner_tag = vec!["p".to_string(), owner_hex];
    owner_tag.extend(rooms::MODERATION_KINDS.map(|kind| kind.to_string()));
    let mut expected = vec![
        vec!["p".to_string(), titled_hex.clone(), "9005".into()],
        vec![
            "p".to_string(),
            moderator_hex.clone(),
            "9005".into(),
            "9011".into(),
        ],
    ];
    expected.sort();
    expected.insert(0, owner_tag);
    assert_eq!(admins, expected);
    let mut members: Vec<Vec<String>> = f
        .published(kinds::MEMBERS, "room")
        .await
        .unwrap()
        .tags
        .find_all("p")
        .map(|tag| tag.as_slice().to_vec())
        .collect();
    members.sort();
    let mut expected = vec![
        vec!["p".to_string(), titled_hex.clone(), "admin".into()],
        vec!["p".to_string(), moderator_hex.clone()],
    ];
    expected.sort();
    assert_eq!(members, expected);

    // Revoking a kind takes it back, and leaves the member in the room.
    f.publish(
        &f.owner,
        kinds::UNASSIGN_PERMISSION,
        &[&["h", "room"], &["p", &moderator_hex, "9005"]],
    )
    .await
    .unwrap();
    assert_eq!(
        f.publish(&moderator, kinds::DELETE_EVENT, &delete)
            .await
            .map(drop),
        refused
    );
    assert!(
        f.snapshot().await.rooms["room"]
            .members
            .contains(&moderator.public_key())
    );
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

    // What a moderator deleted stays deleted, and only in its room.
    assert_eq!(f.store(&here).await, Saved::Deleted);
    assert_eq!(f.store(&there).await, Saved::Duplicate);
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

    f.publish(&f.owner, kinds::JOIN_REQUEST, &[&["h", "room"]])
        .await
        .unwrap();

    f.publish(&f.owner, kinds::DELETE_GROUP, &[&["h", "room"]])
        .await
        .unwrap();

    assert!(f.snapshot().await.rooms.is_empty());
    assert!(f.published(kinds::PINS, "room").await.is_none());
    assert!(f.published(kinds::METADATA, "room").await.is_none());
    assert!(f.published(kinds::MEMBERS, "room").await.is_none());
    assert!(!f.exists(&message).await);

    // Only the deletion is left, and the room can be created again from nothing.
    let left = caravel_relays::events::queries::query_events(
        f.core.db.pool(),
        &f.relay.id,
        &Filter::new().add_tag(coracle_lib::filters::TagMatch::Any, "h", "room"),
        10,
        f.now(),
    )
    .await?;
    assert_eq!(
        left.iter()
            .map(|stored| stored.head.kind)
            .collect::<Vec<_>>(),
        [kinds::DELETE_GROUP]
    );
    f.publish(&f.owner, kinds::CREATE_GROUP, &[&["h", "room"]])
        .await
        .unwrap();
    assert!(f.snapshot().await.rooms["room"].members.is_empty());
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
        snapshot.check_room_write(&event),
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
        Call::CreateRole(caravel_relays::access::Role {
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
        kinds::ASSIGN_PERMISSION,
        &[
            &["h", "room"],
            &["p", &bob.public_key().to_hex(), "9001", "9005"],
        ],
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
        .with_tx(async |conn| {
            caravel_billing::relays::commands::rebuild_derived(conn, &f.key, &f.relay.id, now).await
        })
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
    caravel_relays::events::queries::query_events(
        f.core.db.pool(),
        &f.relay.id,
        &filter,
        1000,
        f.now(),
    )
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
