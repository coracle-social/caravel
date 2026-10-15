//! A relay's rules, ported from zooid's instance tests: who may write, who may
//! subscribe, who may see which event, and who gets signatures.

use caravel_core::relay_policy::{
    self, CREATED_AT_UPPER_LIMIT, EventFacts, Refusal, RelaySnapshot, admit_req, admit_write,
    can_receive, kinds, strips_signature,
};
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
        self_pubkey: relay.public_key().to_hex(),
        owner: owner.public_key().to_hex(),
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
        kinds::BLOB_DESCRIPTOR,
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
    let note = facts(1, &f.snapshot.owner);
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
    assert_eq!(
        admit_req(&public, &[hex(&f.stranger)]),
        Err(Refusal::blocked("you have been banned from this relay"))
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
        kinds::BLOB_DESCRIPTOR,
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
