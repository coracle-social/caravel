//! A relay's rules, ported from zooid's instance tests: who may write, who may
//! subscribe, who may see which event, and who gets signatures.

use caravel_core::events::{EventGroup, kinds};
use caravel_core::relay_policy::{
    self, CREATED_AT_UPPER_LIMIT, EventFacts, Refusal, RelaySnapshot, RoomSnapshot, admit_req,
    admit_write, can_receive, check_room_write, strips_signature,
};
use caravel_core::relays::url_host;
use caravel_core::rooms::kinds as room_kinds;
use caravel_core::testing::sign_event;
use coracle_lib::keys::SecretKey;

const NOW: i64 = 1_800_000_000;

struct Fixture {
    snapshot: RelaySnapshot,
    owner: SecretKey,
    relay: SecretKey,
    member: SecretKey,
    stranger: SecretKey,
}

fn fixture() -> Fixture {
    let owner = SecretKey::generate();
    let relay = SecretKey::generate();
    let member = SecretKey::generate();
    let stranger = SecretKey::generate();

    let snapshot = RelaySnapshot {
        relay_id: "test".to_string(),
        relay_pubkey: relay.public_key().to_hex(),
        tenant_pubkey: owner.public_key().to_hex(),
        members: [member.public_key().to_hex()].into_iter().collect(),
        ..Default::default()
    };

    Fixture {
        snapshot,
        owner,
        relay,
        member,
        stranger,
    }
}

fn facts(kind: u16, pubkey: &str) -> EventFacts<'_> {
    EventFacts {
        kind,
        pubkey,
        room: None,
    }
}

fn hex(secret: &SecretKey) -> String {
    secret.public_key().to_hex()
}

#[test]
fn members_and_managers_may_write_and_strangers_may_not() {
    let f = fixture();

    for author in [&f.owner, &f.member] {
        let event = sign_event(author, 1, "hi", &[], NOW);
        assert_eq!(
            admit_write(&f.snapshot, &event, &[hex(author)], NOW),
            Ok(())
        );
    }

    let event = sign_event(&f.stranger, 1, "hi", &[], NOW);
    assert_eq!(
        admit_write(&f.snapshot, &event, &[hex(&f.stranger)], NOW),
        Err(Refusal::restricted("you are not a member of this relay"))
    );

    let open = RelaySnapshot {
        public_write: true,
        ..f.snapshot.clone()
    };
    assert_eq!(admit_write(&open, &event, &[hex(&f.stranger)], NOW), Ok(()));
}

#[test]
fn writing_needs_authentication_as_the_author() {
    let f = fixture();
    let event = sign_event(&f.member, 1, "hi", &[], NOW);

    assert_eq!(
        admit_write(&f.snapshot, &event, &[], NOW),
        Err(Refusal::AuthRequired(
            "authentication is required for access"
        ))
    );
    assert_eq!(
        admit_write(&f.snapshot, &event, &[hex(&f.owner)], NOW),
        Err(Refusal::restricted(
            "you cannot publish events on behalf of others"
        ))
    );

    // The relay's own events may be delivered by anyone authenticated.
    let relay_event = sign_event(&f.relay, 1, "from the relay", &[], NOW);
    assert_eq!(
        admit_write(&f.snapshot, &relay_event, &[hex(&f.member)], NOW),
        Ok(())
    );
}

#[test]
fn a_gift_wrap_or_zap_for_a_member_needs_no_authentication() {
    let f = fixture();
    let member = hex(&f.member);

    let wrap = sign_event(
        &f.stranger,
        kinds::GIFT_WRAP,
        "sealed",
        &[&["p", &member]],
        NOW,
    );
    assert_eq!(admit_write(&f.snapshot, &wrap, &[], NOW), Ok(()));

    let zap = sign_event(&f.stranger, kinds::ZAP_RECEIPT, "", &[&["p", &member]], NOW);
    assert_eq!(admit_write(&f.snapshot, &zap, &[], NOW), Ok(()));

    let elsewhere = sign_event(
        &f.stranger,
        kinds::GIFT_WRAP,
        "sealed",
        &[&["p", &hex(&f.stranger)]],
        NOW,
    );
    assert!(admit_write(&f.snapshot, &elsewhere, &[], NOW).is_err());
}

#[test]
fn events_dated_too_far_ahead_are_refused() {
    let f = fixture();
    let author = [hex(&f.member)];

    let edge = sign_event(&f.member, 1, "", &[], NOW + CREATED_AT_UPPER_LIMIT);
    assert_eq!(admit_write(&f.snapshot, &edge, &author, NOW), Ok(()));

    let beyond = sign_event(&f.member, 1, "", &[], NOW + CREATED_AT_UPPER_LIMIT + 1);
    assert_eq!(
        admit_write(&f.snapshot, &beyond, &author, NOW),
        Err(Refusal::invalid(
            "this event is dated too far in the future"
        ))
    );
}

#[test]
fn kinds_the_relay_writes_itself_are_refused() {
    let f = fixture();
    for kind in [
        kinds::RELAY_ADD_MEMBER,
        kinds::RELAY_REMOVE_MEMBER,
        kinds::RELAY_MEMBERS,
        kinds::RELAY_ROLE,
        kinds::BLOSSOM_AUTH,
    ] {
        let event = sign_event(&f.owner, kind, "", &[], NOW);
        assert_eq!(
            admit_write(&f.snapshot, &event, &[hex(&f.owner)], NOW),
            Err(Refusal::invalid("this event's kind is not accepted")),
            "kind {kind}"
        );
    }
}

#[test]
fn banned_authors_and_events_are_blocked() {
    let mut f = fixture();
    let event = sign_event(&f.member, 1, "", &[], NOW);
    let author = [hex(&f.member)];

    f.snapshot.banned_events.insert(event.id.to_hex());
    assert_eq!(
        admit_write(&f.snapshot, &event, &author, NOW),
        Err(Refusal::blocked(
            "this event has been banned from this relay"
        ))
    );

    f.snapshot.banned_events.clear();
    f.snapshot.banned_pubkeys.insert(hex(&f.member));
    assert_eq!(
        admit_write(&f.snapshot, &event, &author, NOW),
        Err(Refusal::blocked(
            "event author has been banned from this relay"
        ))
    );
}

#[test]
fn a_protected_event_is_published_only_by_its_authenticated_author() {
    let f = fixture();
    let event = sign_event(&f.member, 1, "", &[&["-"]], NOW);

    assert!(matches!(
        admit_write(&f.snapshot, &event, &[], NOW),
        Err(Refusal::AuthRequired(_))
    ));
    assert_eq!(
        admit_write(&f.snapshot, &event, &[hex(&f.owner)], NOW),
        Err(Refusal::blocked("must be published by event author"))
    );
    assert_eq!(
        admit_write(&f.snapshot, &event, &[hex(&f.member)], NOW),
        Ok(())
    );

    let repost = sign_event(
        &f.member,
        kinds::REPOST,
        &serde_json::to_string(&event).unwrap(),
        &[],
        NOW,
    );
    assert_eq!(
        admit_write(&f.snapshot, &repost, &[hex(&f.member)], NOW),
        Err(Refusal::blocked("can't repost nip70 protected"))
    );
}

#[test]
fn reading_needs_authentication_unless_anonymous_reads_are_on() {
    let f = fixture();
    assert!(matches!(
        admit_req(&f.snapshot, &[]),
        Err(Refusal::AuthRequired(_))
    ));

    let anonymous = RelaySnapshot {
        anonymous_read: true,
        ..f.snapshot.clone()
    };
    assert_eq!(admit_req(&anonymous, &[]), Ok(()));
    let note = facts(1, &f.snapshot.tenant_pubkey);
    assert!(can_receive(&anonymous, None, note));
    assert!(!can_receive(&f.snapshot, None, note));
}

#[test]
fn a_private_relay_is_read_only_by_its_members() {
    let f = fixture();
    assert_eq!(admit_req(&f.snapshot, &[hex(&f.member)]), Ok(()));
    assert_eq!(
        admit_req(&f.snapshot, &[hex(&f.stranger)]),
        Err(Refusal::restricted("you are not a member of this relay"))
    );
    // One proven member is enough for a connection holding several keys.
    assert_eq!(
        admit_req(&f.snapshot, &[hex(&f.stranger), hex(&f.member)]),
        Ok(())
    );

    let public = RelaySnapshot {
        public_read: true,
        banned_pubkeys: [hex(&f.stranger)].into_iter().collect(),
        ..f.snapshot.clone()
    };
    assert_eq!(admit_req(&public, &[hex(&f.member)]), Ok(()));
    assert_eq!(admit_req(&public, &[pubkey_of_nobody()]), Ok(()));
    assert_eq!(
        admit_req(&public, &[hex(&f.stranger)]),
        Err(Refusal::blocked("you have been banned from this relay"))
    );
}

fn pubkey_of_nobody() -> String {
    SecretKey::generate().public_key().to_hex()
}

/// A snapshot holding an open, a private and a hidden room, each with `member`
/// in it, and a closed one `member` was invited to with a code.
fn rooms_fixture(f: &Fixture) -> RelaySnapshot {
    let member = hex(&f.member);
    let room = |hidden, private, closed| RoomSnapshot {
        hidden,
        private,
        closed,
        members: [member.clone()].into_iter().collect(),
        invites: ["sesame".to_string()].into_iter().collect(),
        ..Default::default()
    };
    let mut snapshot = RelaySnapshot {
        groups_enabled: true,
        anonymous_read: true,
        ..f.snapshot.clone()
    };
    snapshot
        .rooms
        .insert("open".into(), room(false, false, false));
    snapshot
        .rooms
        .insert("private".into(), room(false, true, false));
    snapshot
        .rooms
        .insert("hidden".into(), room(true, false, false));
    snapshot
        .rooms
        .insert("closed".into(), room(false, false, true));
    snapshot
}

fn in_room<'a>(kind: u16, pubkey: &'a str, room: &'a str) -> EventFacts<'a> {
    EventFacts {
        kind,
        pubkey,
        room: Some(room),
    }
}

#[test]
fn rooms_decide_who_sees_their_events() {
    let f = fixture();
    let snapshot = rooms_fixture(&f);
    let (owner, member) = (hex(&f.owner), hex(&f.member));
    let outsider = pubkey_of_nobody();
    let readers = |event: EventFacts<'_>| {
        [
            can_receive(&snapshot, Some(&owner), event),
            can_receive(&snapshot, Some(&member), event),
            can_receive(&snapshot, Some(&outsider), event),
            can_receive(&snapshot, None, event),
        ]
    };

    // Owner, member, outsider and anonymous reader, in that order.
    assert_eq!(readers(in_room(9, &member, "open")), [true; 4]);
    assert_eq!(
        readers(in_room(9, &member, "private")),
        [true, true, false, false]
    );
    assert_eq!(
        readers(in_room(room_kinds::METADATA, &owner, "private")),
        [true; 4],
        "a private room shows outsiders its metadata"
    );
    assert_eq!(
        readers(in_room(room_kinds::MEMBERS, &owner, "private")),
        [true, true, false, false]
    );
    assert_eq!(
        readers(in_room(room_kinds::METADATA, &owner, "hidden")),
        [true, true, false, false],
        "a hidden room shows outsiders nothing"
    );
    assert_eq!(
        readers(in_room(9, &member, "hidden")),
        [true, true, false, false]
    );

    // A room nobody created shows outsiders only its deletion.
    assert_eq!(
        readers(in_room(9, &member, "ghost")),
        [true, false, false, false]
    );
    assert_eq!(
        readers(in_room(room_kinds::DELETE_GROUP, &owner, "ghost")),
        [true; 4]
    );

    // Rooms that are off don't hide anything.
    let off = RelaySnapshot {
        groups_enabled: false,
        ..snapshot.clone()
    };
    assert!(can_receive(
        &off,
        Some(&outsider),
        in_room(9, &member, "hidden")
    ));
}

#[test]
fn a_count_includes_only_what_the_reader_may_see() {
    let f = fixture();
    let snapshot = rooms_fixture(&f);
    let (owner, member) = (hex(&f.owner), hex(&f.member));
    let outsider = pubkey_of_nobody();
    let group = |kind, author: &SecretKey, d: Option<&str>, h: Option<&str>, count| EventGroup {
        kind,
        pubkey: author.public_key().as_bytes(),
        d: d.map(str::to_string),
        h: h.map(str::to_string),
        count,
    };
    let groups = [
        group(1, &f.member, None, None, 7),
        group(9, &f.member, None, Some("open"), 2),
        group(9, &f.member, None, Some("private"), 3),
        group(room_kinds::METADATA, &f.relay, Some("private"), None, 1),
        group(9, &f.member, None, Some("hidden"), 4),
        group(room_kinds::CREATE_INVITE, &f.owner, None, Some("open"), 5),
        group(kinds::PUSH_SUBSCRIPTION, &f.member, Some("phone"), None, 1),
    ];
    let count = |authed: &[String]| relay_policy::count_visible(&snapshot, authed, &groups);

    assert_eq!(count(std::slice::from_ref(&owner)), 17);
    assert_eq!(count(std::slice::from_ref(&member)), 18);
    assert_eq!(count(std::slice::from_ref(&outsider)), 10);
    assert_eq!(count(&[]), 10, "an anonymous reader counts as an outsider");
    assert_eq!(count(&[outsider, member]), 18);
}

#[test]
fn a_closed_room_takes_posts_only_from_its_members() {
    let f = fixture();
    let snapshot = rooms_fixture(&f);
    let outsider = SecretKey::generate();
    let post = |author: &SecretKey, kind: u16, tags: &[&[&str]]| {
        check_room_write(&snapshot, &sign_event(author, kind, "", tags, NOW))
    };

    assert_eq!(post(&f.member, 9, &[&["h", "closed"]]), Ok(()));
    assert_eq!(post(&f.owner, 9, &[&["h", "closed"]]), Ok(()));
    assert_eq!(
        post(&outsider, 9, &[&["h", "closed"]]),
        Err(Refusal::restricted("you are not a member of that group"))
    );

    // Anyone may ask to join; the invite decides whether they get in.
    assert_eq!(
        post(&outsider, room_kinds::JOIN_REQUEST, &[&["h", "closed"]]),
        Ok(())
    );
    assert_eq!(
        post(
            &outsider,
            room_kinds::JOIN_REQUEST,
            &[&["h", "closed"], &["code", "sesame"]]
        ),
        Ok(())
    );

    // Posting needs a room that exists, and nobody moderates one but its moderators.
    assert_eq!(
        post(&f.member, 9, &[&["h", "nowhere"]]),
        Err(Refusal::invalid("group not found"))
    );
    assert_eq!(
        post(
            &f.member,
            room_kinds::EDIT_METADATA,
            &[&["h", "open"], &["name", "Mine"]]
        ),
        Err(Refusal::restricted(
            "you are not authorized to manage groups"
        ))
    );
    assert_eq!(
        post(&f.member, room_kinds::CREATE_GROUP, &[&["h", "mine"]]),
        Err(Refusal::restricted(
            "you are not authorized to manage groups"
        ))
    );
    assert_eq!(
        post(&f.owner, room_kinds::CREATE_GROUP, &[&["h", "mine"]]),
        Ok(())
    );
}

#[test]
fn credentials_and_requests_are_never_served() {
    let f = fixture();
    let owner = hex(&f.owner);

    for kind in [
        kinds::RELAY_INVITE,
        caravel_core::rooms::kinds::CREATE_INVITE,
        kinds::RELAY_JOIN,
        kinds::RELAY_LEAVE,
        kinds::BLOSSOM_AUTH,
    ] {
        assert!(
            !can_receive(&f.snapshot, Some(&owner), facts(kind, &owner)),
            "kind {kind}"
        );
    }

    // A push subscription comes back to its author and nobody else.
    let member = hex(&f.member);
    let subscription = facts(kinds::PUSH_SUBSCRIPTION, &member);
    assert!(can_receive(&f.snapshot, Some(&member), subscription));
    assert!(!can_receive(&f.snapshot, Some(&owner), subscription));
}

#[test]
fn stripping_spares_whoever_manages_the_relay() {
    let f = fixture();
    let stripping = RelaySnapshot {
        strip_signatures: true,
        ..f.snapshot.clone()
    };

    assert!(strips_signature(&stripping, &[hex(&f.member)]));
    assert!(strips_signature(&stripping, &[]));
    assert!(!strips_signature(&stripping, &[hex(&f.owner)]));
    assert!(!strips_signature(&f.snapshot, &[hex(&f.member)]));
}

#[test]
fn ephemeral_kinds_are_not_stored() {
    assert!(!relay_policy::is_stored(20001));
    assert!(relay_policy::is_stored(1));
    assert!(relay_policy::is_stored(30023));
}

#[test]
fn relay_urls_name_their_host_without_scheme_port_or_path() {
    assert_eq!(url_host("ws://Relay.Example:8080/path"), "relay.example");
    assert_eq!(url_host("relay.example."), "relay.example");
}

#[test]
fn a_push_subscription_may_name_any_of_the_relays_hosts() {
    let author = SecretKey::generate();
    let hosts = ["relay.example".to_string(), "old.example".to_string()];
    let subscription = |relay: &str| {
        sign_event(
            &author,
            kinds::PUSH_SUBSCRIPTION,
            "",
            &[
                &["d", "phone"],
                &["relay", relay],
                &["filter", "{}"],
                &["callback", "https://push.example/"],
            ],
            NOW,
        )
    };

    for relay in [
        "wss://relay.example",
        "wss://Old.Example:443/",
        "old.example",
    ] {
        assert_eq!(
            relay_policy::check_push_subscription(&subscription(relay), &hosts, 0),
            Ok(()),
            "{relay}"
        );
    }
    assert!(
        relay_policy::check_push_subscription(&subscription("wss://other.example"), &hosts, 0)
            .is_err()
    );
}

#[test]
fn a_push_subscription_says_what_is_wrong_with_it() {
    let author = SecretKey::generate();
    let hosts = ["relay.example".to_string()];
    let relay = ["relay", "wss://relay.example"];
    let filter = ["filter", "{\"kinds\":[1]}"];
    let callback = ["callback", "https://push.example/"];
    let check = |tags: &[&[&str]]| {
        let event = sign_event(&author, kinds::PUSH_SUBSCRIPTION, "", tags, NOW);
        relay_policy::check_push_subscription(&event, &hosts, 0)
            .map_err(|refusal| refusal.message())
    };

    assert_eq!(
        check(&[&relay, &filter, &callback]),
        Err("invalid: missing or empty d tag".into())
    );
    assert_eq!(
        check(&[&["d", ""], &relay, &filter, &callback]),
        Err("invalid: missing or empty d tag".into())
    );
    assert_eq!(
        check(&[&["d", "a"], &filter, &callback]),
        Err("invalid: missing relay tag".into())
    );
    assert_eq!(
        check(&[&["d", "a"], &relay, &callback]),
        Err("invalid: at least one filter tag is required".into())
    );
    assert!(
        check(&[&["d", "a"], &relay, &["filter", "{nope"], &callback])
            .unwrap_err()
            .starts_with("invalid: filter tag contains invalid JSON")
    );
    assert!(
        check(&[&["d", "a"], &relay, &filter, &["ignore", "[]"], &callback])
            .unwrap_err()
            .starts_with("invalid: filter tag contains invalid JSON")
    );
    assert_eq!(
        check(&[&["d", "a"], &relay, &filter]),
        Err("invalid: missing callback tag".into())
    );
    assert_eq!(
        check(&[&["d", "a"], &relay, &filter, &callback, &callback]),
        Err("invalid: too many callback tags".into())
    );
    assert_eq!(
        check(&[&["d", "a"], &relay, &filter, &["callback", ""]]),
        Err("invalid: empty callback tag".into())
    );
    assert_eq!(
        check(&[
            &["d", "a"],
            &relay,
            &filter,
            &["callback", "ftp://push.example/"]
        ]),
        Err("invalid: callback must be a valid HTTP or HTTPS URL".into())
    );
    assert_eq!(
        check(&[&["d", "a"], &relay, &filter, &["ignore", "{}"], &callback]),
        Ok(())
    );
}
