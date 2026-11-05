//! Relay access, ported from zooid's management tests: members, bans, roles,
//! method grants and claims, and the events the relay publishes about them.

use anyhow::Result;
use caravel_billing::relays::Relay;
use caravel_core::testing::{create_relay, sign_event};
use caravel_core::{Core, management, testing};
use caravel_relays::access::MemberSource;
use caravel_relays::access::Role;
use caravel_relays::events::Saved;
use caravel_relays::management::MANAGED_METHODS;
use caravel_relays::management::{Call, Outcome};
use caravel_util::error::Error;
use caravel_util::nostr::Key;
use caravel_util::nostr::kinds;
use coracle_lib::events::Event;
use coracle_lib::filters::Filter;
use coracle_lib::keys::{PublicKey, SecretKey};
use serde_json::json;

struct Fixture {
    core: Core,
    relay: Relay,
    key: Key,
    owner: String,
}

async fn fixture() -> Fixture {
    let core = testing::core(&SecretKey::generate().public_key().to_hex()).await;
    let (relay, owner) = create_relay(&core).await;
    let key = core.relay_key(&relay).unwrap();
    Fixture {
        core,
        relay,
        key,
        owner: owner.public_key().to_hex(),
    }
}

fn pubkey() -> String {
    SecretKey::generate().public_key().to_hex()
}

impl Fixture {
    async fn call(&self, call: Call) -> Result<serde_json::Value> {
        Ok(management::call(&self.core, &self.relay, call).await?)
    }

    async fn members(&self) -> Result<Vec<String>> {
        Ok(
            caravel_relays::access::queries::list_members(self.core.db.pool(), &self.relay.id)
                .await?
                .into_iter()
                .map(|member| member.pubkey)
                .collect(),
        )
    }

    /// The latest event the relay published of a kind.
    async fn published(&self, kind: u16) -> Result<Option<Event>> {
        let filter = Filter::new()
            .add_kind(kind)
            .add_author(self.key.pubkey().parse()?);
        let found = caravel_relays::events::queries::query_events(
            self.core.db.pool(),
            &self.relay.id,
            &filter,
            1,
            self.core.clock.now(),
            &[],
        )
        .await?;
        Ok(found
            .first()
            .map(|stored| serde_json::from_str(&stored.to_json(false)).unwrap()))
    }

    /// The `member` tags of the relay's member list.
    async fn member_tags(&self) -> Result<Vec<Vec<String>>> {
        let list = self
            .published(kinds::RELAY_MEMBERS)
            .await?
            .expect("no member list");
        Ok(list
            .tags
            .find_all("member")
            .map(|tag| tag.as_slice().to_vec())
            .collect())
    }
}

#[tokio::test]
async fn the_owner_is_a_member_from_the_start() -> Result<()> {
    let f = fixture().await;
    assert_eq!(f.members().await?, vec![f.owner.clone()]);
    assert_eq!(
        f.member_tags().await?,
        vec![vec!["member".to_string(), f.owner.clone()]]
    );

    let member =
        caravel_relays::access::queries::get_member(f.core.db.pool(), &f.relay.id, &f.owner)
            .await?
            .unwrap();
    assert_eq!(member.source, MemberSource::Owner);
    Ok(())
}

#[tokio::test]
async fn membership_changes_are_idempotent_and_published() -> Result<()> {
    let f = fixture().await;
    let alice = pubkey();
    let now = f.core.clock.now();

    let added = f
        .core
        .db
        .with_tx(async |conn| {
            caravel_relays::access::commands::admit(
                conn,
                &f.key,
                &f.relay.id,
                &alice,
                MemberSource::Manual,
                now,
            )
            .await
        })
        .await?;
    assert_eq!(
        added.iter().map(|event| event.kind).collect::<Vec<_>>(),
        vec![8000, 13534]
    );
    assert!(
        added
            .iter()
            .all(|event| event.verify().is_ok() && event.tags.has("-"))
    );

    let again = f
        .core
        .db
        .with_tx(async |conn| {
            caravel_relays::access::commands::admit(
                conn,
                &f.key,
                &f.relay.id,
                &alice,
                MemberSource::Claim,
                now,
            )
            .await
        })
        .await?;
    assert!(again.is_empty());
    assert_eq!(f.members().await?.len(), 2);

    f.call(Call::UnallowPubkey(alice.clone())).await?;
    f.call(Call::UnallowPubkey(alice.clone())).await?;
    assert_eq!(f.members().await?, vec![f.owner.clone()]);
    assert!(f.published(kinds::RELAY_REMOVE_MEMBER).await?.is_some());
    Ok(())
}

/// The error code a failed call answers with, if it carries one.
fn refusal_code(refused: &Result<serde_json::Value>) -> Option<&str> {
    match refused.as_ref().err()?.downcast_ref::<Error>() {
        Some(Error::Conflict { code, .. } | Error::Invalid { code, .. }) => Some(code),
        _ => None,
    }
}

/// A free relay seats ten, the owner included; a management call may not add an
/// eleventh, neither may assigning a role, and nor may granting a method, which
/// admits as it goes.
#[tokio::test]
async fn adding_members_stops_at_the_plans_limit() -> Result<()> {
    let f = fixture().await;
    for _ in 0..9 {
        f.call(Call::AllowPubkey(pubkey())).await?;
    }

    let refused = f.call(Call::AllowPubkey(pubkey())).await;
    assert_eq!(refusal_code(&refused), Some("member-limit"));

    let outsider = pubkey();
    f.call(Call::CreateRole(caravel_relays::access::Role {
        id: "greeter".into(),
        label: "Greeter".into(),
        description: String::new(),
        hue: 120,
        position: 0,
    }))
    .await?;
    let refused = f.call(Call::AssignRole(outsider, "greeter".into())).await;
    assert_eq!(refusal_code(&refused), Some("member-limit"));

    let refused = f
        .call(Call::AssignMethod(pubkey(), "banpubkey".into()))
        .await;
    assert_eq!(refusal_code(&refused), Some("member-limit"));

    // The dashboard adds through the same rule.
    let refused = management::add_member(&f.core, &f.relay, &pubkey()).await;
    assert!(matches!(
        refused,
        Err(Error::Conflict {
            code: "member-limit",
            ..
        })
    ));
    assert_eq!(f.members().await?.len(), 10);
    Ok(())
}

#[tokio::test]
async fn the_owner_can_be_neither_removed_nor_banned() -> Result<()> {
    let f = fixture().await;
    let refused = f.call(Call::UnallowPubkey(f.owner.clone())).await;
    assert!(refused.is_err());
    let refused = f.call(Call::BanPubkey(f.owner.clone(), "no".into())).await;
    assert!(refused.is_err());
    assert_eq!(f.members().await?, vec![f.owner.clone()]);
    Ok(())
}

#[tokio::test]
async fn banning_a_pubkey_removes_it_and_everything_it_wrote() -> Result<()> {
    let f = fixture().await;
    let spammer = SecretKey::generate();
    let spammer_hex = spammer.public_key().to_hex();
    f.call(Call::AllowPubkey(spammer_hex.clone())).await?;

    let now = f.core.clock.now();
    let spam = sign_event(&spammer, 1, "spam", &[], now);
    f.core
        .db
        .with_tx(async |conn| {
            caravel_relays::events::commands::save_event(conn, &f.relay.id, &spam, now).await
        })
        .await?;

    f.call(Call::BanPubkey(spammer_hex.clone(), "spam".into()))
        .await?;

    assert!(!f.members().await?.contains(&spammer_hex));
    assert_eq!(
        f.call(Call::ListBannedPubkeys).await?,
        json!([{"pubkey": spammer_hex, "reason": "spam"}])
    );
    let written = caravel_relays::events::queries::query_events(
        f.core.db.pool(),
        &f.relay.id,
        &Filter::new().add_author(spammer.public_key()),
        10,
        now,
        &[],
    )
    .await?;
    assert!(written.is_empty());

    // Allowing lets them back in and lifts the ban.
    f.call(Call::AllowPubkey(spammer_hex.clone())).await?;
    assert!(f.members().await?.contains(&spammer_hex));
    assert_eq!(f.call(Call::ListBannedPubkeys).await?, json!([]));
    Ok(())
}

// A ban takes what a pubkey wrote, but not its tombstones, so what it deleted
// stays deleted once the ban is lifted.
#[tokio::test]
async fn a_ban_keeps_the_pubkeys_deletions_and_requests_to_vanish() -> Result<()> {
    let f = fixture().await;
    let author = SecretKey::generate();
    let author_hex = author.public_key().to_hex();
    let now = f.core.clock.now();
    let store = async |event: &Event| {
        caravel_core::ingest::store(&f.core, &f.relay, &f.key, false, event)
            .await
            .map(|(saved, _)| saved)
    };

    let vanished = sign_event(&author, 1, "vanished", &[], now - 20);
    let vanish = sign_event(&author, 62, "", &[&["relay", "ALL_RELAYS"]], now - 15);
    let deleted = sign_event(&author, 1, "deleted", &[], now - 10);
    let deletion = sign_event(&author, 5, "", &[&["e", &deleted.id.to_hex()]], now - 5);
    for event in [&vanished, &vanish, &deleted, &deletion] {
        store(event).await?;
    }

    f.call(Call::BanPubkey(author_hex.clone(), "spam".into()))
        .await?;
    f.call(Call::UnbanPubkey(author_hex)).await?;

    let kept: Vec<[u8; 32]> = caravel_relays::events::queries::query_events(
        f.core.db.pool(),
        &f.relay.id,
        &Filter::new().add_author(author.public_key()),
        10,
        now,
        &[],
    )
    .await?
    .iter()
    .map(|stored| stored.head.id)
    .collect();
    assert_eq!(kept, vec![deletion.id.to_bytes(), vanish.id.to_bytes()]);
    assert_eq!(store(&deleted).await?, Saved::Deleted);
    assert_eq!(store(&vanished).await?, Saved::Deleted);
    Ok(())
}

#[tokio::test]
async fn unbanning_lifts_the_ban_without_admitting() -> Result<()> {
    let f = fixture().await;
    let target = pubkey();
    f.call(Call::BanPubkey(target.clone(), String::new()))
        .await?;
    f.call(Call::UnbanPubkey(target.clone())).await?;

    assert_eq!(f.call(Call::ListBannedPubkeys).await?, json!([]));
    assert!(!f.members().await?.contains(&target));
    Ok(())
}

#[tokio::test]
async fn banning_an_event_deletes_it_until_it_is_allowed() -> Result<()> {
    let f = fixture().await;
    let author = SecretKey::generate();
    let now = f.core.clock.now();
    let event = sign_event(&author, 1, "bad", &[], now);
    f.core
        .db
        .with_tx(async |conn| {
            caravel_relays::events::commands::save_event(conn, &f.relay.id, &event, now).await
        })
        .await?;

    let id = event.id.to_hex();
    f.call(Call::BanEvent(id.clone(), "bad".into())).await?;
    assert_eq!(
        f.call(Call::ListBannedEvents).await?,
        json!([{"id": id, "reason": "bad"}])
    );
    assert!(
        caravel_relays::events::queries::get_events(
            f.core.db.pool(),
            &f.relay.id,
            &[event.id.to_bytes()]
        )
        .await?
        .is_empty()
    );

    f.call(Call::AllowEvent(id)).await?;
    assert_eq!(f.call(Call::ListBannedEvents).await?, json!([]));
    Ok(())
}

#[tokio::test]
async fn members_are_listed_as_allowed_pubkeys() -> Result<()> {
    let f = fixture().await;
    let alice = pubkey();
    f.call(Call::AllowPubkey(alice.clone())).await?;

    let mut listed = f.call(Call::ListAllowedPubkeys).await?;
    listed
        .as_array_mut()
        .unwrap()
        .sort_by_key(|item| item["pubkey"].as_str().unwrap().to_string());
    let mut expected = [f.owner.clone(), alice];
    expected.sort();
    assert_eq!(
        listed,
        json!(
            expected
                .iter()
                .map(|pubkey| json!({"pubkey": pubkey, "reason": "relay member"}))
                .collect::<Vec<_>>()
        )
    );
    Ok(())
}

#[tokio::test]
async fn the_relays_name_description_and_icon_are_managed() -> Result<()> {
    let f = fixture().await;
    let mut changes = f.core.bus.subscribe();

    assert_eq!(
        f.call(Call::ChangeRelayName("Renamed".into())).await?,
        json!(true)
    );
    f.call(Call::ChangeRelayDescription("About us".into()))
        .await?;
    f.call(Call::ChangeRelayIcon("https://example.com/icon.png".into()))
        .await?;

    let relay =
        caravel_billing::relays::queries::require_relay(f.core.db.pool(), &f.relay.id).await?;
    assert_eq!(relay.info_name, "Renamed");
    assert_eq!(relay.info_description, "About us");
    assert_eq!(relay.info_icon, "https://example.com/icon.png");

    // Each change asks the relay to reload, so its information document follows.
    for _ in 0..3 {
        assert!(matches!(
            changes.try_recv(),
            Ok(caravel_util::bus::Change::RelayChanged { relay_id }) if relay_id == f.relay.id
        ));
    }
    Ok(())
}

#[tokio::test]
async fn roles_are_defined_published_and_assigned() -> Result<()> {
    let f = fixture().await;
    let role = Role {
        id: "mod".into(),
        label: "Moderator".into(),
        description: String::new(),
        hue: 0,
        position: 2,
    };
    f.call(Call::CreateRole(role.clone())).await?;

    // Empty and zero fields are left for the client to default.
    let published = f
        .published(kinds::RELAY_ROLE)
        .await?
        .expect("no role event");
    let names: Vec<&str> = published.tags.iter().map(|tag| tag.name()).collect();
    assert_eq!(names, ["-", "d", "label", "order"]);

    let duplicate = f.call(Call::CreateRole(role.clone())).await;
    assert!(matches!(
        duplicate.unwrap_err().downcast::<Error>()?,
        Error::Conflict { .. }
    ));

    let bad_hue = Role {
        hue: 400,
        ..role.clone()
    };
    assert!(f.call(Call::EditRole(bad_hue)).await.is_err());

    f.call(Call::EditRole(Role {
        label: "Mod".into(),
        hue: 120,
        ..role.clone()
    }))
    .await?;
    let edited = f.published(kinds::RELAY_ROLE).await?.unwrap();
    assert_eq!(edited.tags.value("label"), Some("Mod"));
    assert_eq!(edited.tags.value("color"), Some("120"));

    let alice = pubkey();
    f.call(Call::AssignRole(alice.clone(), "mod".into()))
        .await?;
    assert!(
        f.members().await?.contains(&alice),
        "a role makes its holder a member"
    );
    assert!(
        f.member_tags()
            .await?
            .contains(&vec!["member".into(), alice.clone(), "mod".into()])
    );

    assert!(
        f.call(Call::AssignRole(alice.clone(), "missing".into()))
            .await
            .is_err()
    );

    f.call(Call::UnassignRole(alice.clone(), "mod".into()))
        .await?;
    assert!(
        f.member_tags()
            .await?
            .contains(&vec!["member".into(), alice.clone()])
    );
    Ok(())
}

#[tokio::test]
async fn deleting_a_role_takes_it_from_its_holders_and_publishes_a_deletion() -> Result<()> {
    let f = fixture().await;
    let role = Role {
        id: "temp".into(),
        ..Default::default()
    };
    f.call(Call::CreateRole(role.clone())).await?;
    let alice = pubkey();
    f.call(Call::AssignRole(alice.clone(), "temp".into()))
        .await?;

    let Outcome::AccessChanged(published) =
        management::run_call(&f.core, &f.relay, Call::DeleteRole("temp".into())).await?
    else {
        panic!("deleting a role changes access");
    };
    let deletion = published
        .iter()
        .find(|event| event.kind == 5)
        .expect("no deletion");
    assert_eq!(
        deletion.tags.value("a"),
        Some(format!("{}:{}:temp", kinds::RELAY_ROLE, f.key.pubkey()).as_str())
    );
    assert!(f.published(kinds::RELAY_ROLE).await?.is_none());
    assert!(
        f.member_tags()
            .await?
            .contains(&vec!["member".into(), alice])
    );

    // The same id can be defined again straight away.
    f.call(Call::CreateRole(role)).await?;
    assert!(f.published(kinds::RELAY_ROLE).await?.is_some());
    Ok(())
}

#[tokio::test]
async fn methods_are_granted_individually_and_to_members_of_inviting_relays() -> Result<()> {
    let f = fixture().await;
    let alice = pubkey();

    f.call(Call::AssignMethod(alice.clone(), "banpubkey".into()))
        .await?;
    assert!(
        f.members().await?.contains(&alice),
        "a method makes its holder a member"
    );

    let snapshot = caravel_core::events::load_snapshot(&f.core, &f.relay).await?;
    let alice_key = PublicKey::from_hex(&alice).unwrap();
    let mut methods = snapshot.methods_for(&alice_key);
    methods.sort();
    // Members of a relay whose members may invite also hold the invite methods.
    assert_eq!(methods, ["banpubkey", "createclaim", "listclaims"]);
    assert!(snapshot.may_call(&alice_key, "supportedmethods"));
    assert!(!snapshot.may_call(&alice_key, "banevent"));
    assert_eq!(
        snapshot
            .methods_for(&PublicKey::from_hex(&f.owner).unwrap())
            .len(),
        MANAGED_METHODS.len()
    );

    let assignees = f.call(Call::ListMethodAssignees).await?;
    let alice_entry = assignees
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["pubkey"] == alice)
        .unwrap();
    assert_eq!(alice_entry["methods"], json!(["banpubkey"]));

    f.call(Call::UnassignMethod(alice.clone(), "banpubkey".into()))
        .await?;
    let snapshot = caravel_core::events::load_snapshot(&f.core, &f.relay).await?;
    assert!(
        !snapshot
            .methods_for(&alice_key)
            .contains(&"banpubkey".to_string())
    );
    Ok(())
}

#[tokio::test]
async fn method_grants_are_checked() -> Result<()> {
    let f = fixture().await;
    assert!(
        f.call(Call::AssignMethod(pubkey(), "launchrockets".into()))
            .await
            .is_err()
    );
    assert!(
        f.call(Call::AssignMethod(f.owner.clone(), "banpubkey".into()))
            .await
            .is_err()
    );
    assert!(
        f.call(Call::AssignMethod(f.key.pubkey(), "banpubkey".into()))
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn removing_a_member_takes_their_methods() -> Result<()> {
    let f = fixture().await;
    let alice = pubkey();
    f.call(Call::AssignMethod(alice.clone(), "banevent".into()))
        .await?;
    f.call(Call::UnallowPubkey(alice.clone())).await?;

    assert!(
        caravel_relays::management::queries::list_method_grants(f.core.db.pool(), &f.relay.id)
            .await?
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn claims_are_created_listed_and_revoked() -> Result<()> {
    let f = fixture().await;
    let pool = f.core.db.pool();

    assert!(f.call(Call::CreateClaim(String::new())).await.is_err());
    f.call(Call::CreateClaim("alpha".into())).await?;
    f.call(Call::CreateClaim("alpha".into())).await?;
    f.call(Call::CreateClaim("beta".into())).await?;
    assert_eq!(f.call(Call::ListClaims).await?, json!(["alpha", "beta"]));
    assert!(caravel_relays::access::queries::is_live_claim(pool, &f.relay.id, "alpha").await?);

    f.call(Call::DeleteClaim("alpha".into())).await?;
    assert_eq!(f.call(Call::ListClaims).await?, json!(["beta"]));
    assert!(!caravel_relays::access::queries::is_live_claim(pool, &f.relay.id, "alpha").await?);
    Ok(())
}

#[tokio::test]
async fn calls_decode_from_their_wire_params() {
    let pk = "a".repeat(64);
    assert_eq!(
        Call::decode("banpubkey", &[json!(pk), json!("why")]).unwrap(),
        Call::BanPubkey(pk.clone(), "why".into())
    );
    assert_eq!(
        Call::decode(
            "createrole",
            &[json!("r"), json!("R"), json!(""), json!(10), json!(1)]
        )
        .unwrap(),
        Call::CreateRole(Role {
            id: "r".into(),
            label: "R".into(),
            description: String::new(),
            hue: 10,
            position: 1,
        })
    );
    assert!(Call::decode("banpubkey", &[json!("nope")]).is_err());
    assert!(Call::decode("createrole", &[json!("r")]).is_err());
    assert!(Call::decode("listeventsneedingmoderation", &[]).is_err());
}
