//! The event store, ported from zooid's event store tests: one table shared by
//! every relay, replaceable slots, tombstones, expiration and query semantics.

use std::collections::BTreeSet;

use anyhow::Result;
use caravel_core::events::{self, Saved, StoredEvent};
use caravel_core::testing::{self, create_relay, sign_event};
use caravel_core::{Core, Error};
use coracle_lib::events::Event;
use coracle_lib::filters::{Filter, TagMatch};
use coracle_lib::keys::SecretKey;
use futures_util::future::join_all;

const NOW: i64 = 1_800_000_000;

async fn core() -> Core {
    let core = testing::core(&SecretKey::generate().public_key().to_hex()).await;
    core.clock.pin(NOW);
    core
}

async fn save(core: &Core, relay_id: &str, event: &Event) -> Result<Saved> {
    let saved = core
        .db
        .with_tx(async |conn| {
            let saved = events::save_event(&mut *conn, relay_id, event, NOW).await?;
            if saved == Saved::New {
                match event.kind {
                    events::DELETION => {
                        events::apply_deletion(conn, relay_id, event).await?;
                    }
                    events::VANISH => {
                        events::apply_vanish(
                            conn,
                            relay_id,
                            &event.pubkey.as_bytes(),
                            event.created_at,
                        )
                        .await?;
                    }
                    _ => {}
                }
            }
            Ok::<_, Error>(saved)
        })
        .await?;
    Ok(saved)
}

/// A relay's events matching a filter, leaving out the ones the relay signed about
/// its own members.
async fn query(core: &Core, relay_id: &str, filter: Filter) -> Result<Vec<StoredEvent>> {
    let relay = caravel_core::relays::get_relay(core.db.pool(), relay_id)
        .await?
        .expect("no relay");
    Ok(
        events::query_events(core.db.pool(), relay_id, &filter, 1000, NOW)
            .await?
            .into_iter()
            .filter(|event| event.head.pubkey_hex() != relay.pubkey)
            .collect(),
    )
}

async fn ids(core: &Core, relay_id: &str, filter: Filter) -> Result<Vec<[u8; 32]>> {
    Ok(query(core, relay_id, filter)
        .await?
        .iter()
        .map(|event| event.head.id)
        .collect())
}

fn kinds(kinds: &[u16]) -> Filter {
    Filter {
        kinds: Some(kinds.iter().copied().collect()),
        ..Filter::new()
    }
}

#[tokio::test]
async fn a_saved_event_reads_back_exactly() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let author = SecretKey::generate();
    let event = sign_event(
        &author,
        1,
        "hello \"world\"\n",
        &[&["t", "greeting"], &["p", "x", "wss://r"]],
        NOW,
    );

    assert_eq!(save(&core, &relay.id, &event).await?, Saved::New);
    let stored = query(&core, &relay.id, Filter::new()).await?;

    assert_eq!(stored.len(), 1);
    let parsed: Event = serde_json::from_str(&stored[0].to_json(false))?;
    assert_eq!(parsed, event);
    assert!(parsed.verify().is_ok());
    Ok(())
}

#[tokio::test]
async fn relays_never_see_each_others_events() -> Result<()> {
    let core = core().await;
    let (first, _) = create_relay(&core).await;
    let (second, _) = create_relay(&core).await;
    let author = SecretKey::generate();
    let event = sign_event(&author, 1, "only on the first", &[], NOW);

    save(&core, &first.id, &event).await?;

    assert_eq!(
        ids(&core, &first.id, Filter::new()).await?,
        vec![event.id.to_bytes()]
    );
    assert!(ids(&core, &second.id, Filter::new()).await?.is_empty());

    // The same event is stored independently on another relay.
    assert_eq!(save(&core, &second.id, &event).await?, Saved::New);
    Ok(())
}

#[tokio::test]
async fn queries_match_kinds_authors_ids_tags_and_time() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let alice = SecretKey::generate();
    let bob = SecretKey::generate();

    let note = sign_event(&alice, 1, "a note", &[&["t", "nostr"]], NOW - 30);
    let reaction = sign_event(&bob, 7, "+", &[&["t", "nostr"], &["t", "rust"]], NOW - 20);
    let later = sign_event(&bob, 1, "later", &[], NOW - 10);
    for event in [&note, &reaction, &later] {
        save(&core, &relay.id, event).await?;
    }

    assert_eq!(
        ids(&core, &relay.id, kinds(&[7])).await?,
        vec![reaction.id.to_bytes()]
    );

    let by_author = Filter::new().add_author(alice.public_key());
    assert_eq!(
        ids(&core, &relay.id, by_author).await?,
        vec![note.id.to_bytes()]
    );

    let by_id = Filter::new().add_id(later.id);
    assert_eq!(
        ids(&core, &relay.id, by_id).await?,
        vec![later.id.to_bytes()]
    );

    let any_tag = Filter::new().add_tag(TagMatch::Any, "t", "nostr");
    assert_eq!(
        ids(&core, &relay.id, any_tag).await?,
        vec![reaction.id.to_bytes(), note.id.to_bytes()]
    );

    let every_tag =
        Filter::new()
            .add_tag(TagMatch::All, "t", "nostr")
            .add_tag(TagMatch::All, "t", "rust");
    assert_eq!(
        ids(&core, &relay.id, every_tag).await?,
        vec![reaction.id.to_bytes()]
    );

    let window = Filter::new().add_since(NOW - 25).add_until(NOW - 15);
    assert_eq!(
        ids(&core, &relay.id, window).await?,
        vec![reaction.id.to_bytes()]
    );

    // Newest first.
    assert_eq!(
        ids(&core, &relay.id, Filter::new()).await?,
        vec![
            later.id.to_bytes(),
            reaction.id.to_bytes(),
            note.id.to_bytes()
        ]
    );
    Ok(())
}

#[tokio::test]
async fn an_absent_field_matches_everything_and_an_empty_one_nothing() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let author = SecretKey::generate();
    save(
        &core,
        &relay.id,
        &sign_event(&author, 1, "x", &[&["t", "a"]], NOW),
    )
    .await?;

    assert_eq!(ids(&core, &relay.id, Filter::new()).await?.len(), 1);
    assert!(ids(&core, &relay.id, kinds(&[])).await?.is_empty());

    let empty_ids = Filter {
        ids: Some(BTreeSet::new()),
        ..Filter::new()
    };
    assert!(ids(&core, &relay.id, empty_ids).await?.is_empty());

    // Only single-letter tags are indexed, so a longer name matches nothing.
    let long_tag = Filter::new().add_tag(TagMatch::Any, "title", "x");
    assert!(ids(&core, &relay.id, long_tag).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn limits_cap_results_and_ties_break_on_id() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let author = SecretKey::generate();

    let mut tied: Vec<Event> = (0..5)
        .map(|i| sign_event(&author, 1, &i.to_string(), &[], NOW))
        .collect();
    for event in &tied {
        save(&core, &relay.id, event).await?;
    }
    tied.sort_by_key(|event| event.id.to_bytes());

    let limited = Filter::new().add_limit(2);
    assert_eq!(
        ids(&core, &relay.id, limited).await?,
        tied[..2]
            .iter()
            .map(|event| event.id.to_bytes())
            .collect::<Vec<_>>()
    );

    // A limit of zero asks only for what comes later.
    assert!(
        ids(&core, &relay.id, Filter::new().add_limit(0))
            .await?
            .is_empty()
    );

    // The relay's cap applies whether or not the filter names a limit.
    let capped = events::query_events(core.db.pool(), &relay.id, &Filter::new(), 3, NOW).await?;
    assert_eq!(capped.len(), 3);
    let capped = events::query_events(
        core.db.pool(),
        &relay.id,
        &Filter::new().add_limit(100),
        3,
        NOW,
    )
    .await?;
    assert_eq!(capped.len(), 3);
    Ok(())
}

#[tokio::test]
async fn search_matches_words_in_sequence() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let author = SecretKey::generate();
    let quick = sign_event(&author, 1, "the quick brown fox", &[], NOW);
    save(&core, &relay.id, &quick).await?;
    save(
        &core,
        &relay.id,
        &sign_event(&author, 1, "brown, then quick", &[], NOW),
    )
    .await?;

    let search = |text: &str| Filter {
        search: Some(text.to_string()),
        ..Filter::new()
    };

    assert_eq!(
        ids(&core, &relay.id, search("quick brown")).await?,
        vec![quick.id.to_bytes()]
    );
    assert_eq!(
        ids(&core, &relay.id, search("fox")).await?,
        vec![quick.id.to_bytes()]
    );
    // Syntax a tsquery would interpret is just words here.
    assert!(
        ids(&core, &relay.id, search("quick & !brown"))
            .await
            .is_ok()
    );
    Ok(())
}

#[tokio::test]
async fn a_saved_event_is_a_duplicate_the_second_time() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let event = sign_event(&SecretKey::generate(), 1, "once", &[], NOW);

    let results = join_all((0..8).map(|_| save(&core, &relay.id, &event))).await;
    let saved: Vec<Saved> = results.into_iter().collect::<Result<_>>()?;

    assert_eq!(
        saved.iter().filter(|saved| **saved == Saved::New).count(),
        1
    );
    assert_eq!(
        saved
            .iter()
            .filter(|saved| **saved == Saved::Duplicate)
            .count(),
        7
    );
    Ok(())
}

#[tokio::test]
async fn a_replaceable_event_keeps_one_newest_version_per_slot() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let (other, _) = create_relay(&core).await;
    let author = SecretKey::generate();

    let old = sign_event(&author, 0, "old profile", &[], NOW - 10);
    let new = sign_event(&author, 0, "new profile", &[], NOW);
    assert_eq!(save(&core, &relay.id, &old).await?, Saved::New);
    assert_eq!(save(&core, &relay.id, &new).await?, Saved::New);
    assert_eq!(
        ids(&core, &relay.id, kinds(&[0])).await?,
        vec![new.id.to_bytes()]
    );

    // An older version arriving late, or the current one again, changes nothing.
    assert_eq!(save(&core, &relay.id, &old).await?, Saved::Duplicate);
    assert_eq!(save(&core, &relay.id, &new).await?, Saved::Duplicate);
    assert_eq!(
        ids(&core, &relay.id, kinds(&[0])).await?,
        vec![new.id.to_bytes()]
    );

    // The last of two versions in one second wins.
    let tied = sign_event(&author, 0, "tied profile", &[], NOW);
    assert_eq!(save(&core, &relay.id, &tied).await?, Saved::New);
    assert_eq!(
        ids(&core, &relay.id, kinds(&[0])).await?,
        vec![tied.id.to_bytes()]
    );

    // Another relay's slot is its own.
    assert_eq!(save(&core, &other.id, &old).await?, Saved::New);
    assert_eq!(
        ids(&core, &other.id, kinds(&[0])).await?,
        vec![old.id.to_bytes()]
    );
    Ok(())
}

#[tokio::test]
async fn addressable_events_have_a_slot_per_d_tag() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let author = SecretKey::generate();

    let first = sign_event(&author, 30023, "a", &[&["d", "first"]], NOW - 5);
    let second = sign_event(&author, 30023, "b", &[&["d", "second"]], NOW - 5);
    let first_again = sign_event(&author, 30023, "a2", &[&["d", "first"]], NOW);
    for event in [&first, &second, &first_again] {
        save(&core, &relay.id, event).await?;
    }

    let mut found = ids(&core, &relay.id, kinds(&[30023])).await?;
    found.sort();
    let mut expected = vec![second.id.to_bytes(), first_again.id.to_bytes()];
    expected.sort();
    assert_eq!(found, expected);
    Ok(())
}

#[tokio::test]
async fn racing_versions_of_a_slot_leave_the_newest() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let author = SecretKey::generate();
    let versions: Vec<Event> = (0..10)
        .map(|i| sign_event(&author, 10002, &i.to_string(), &[], NOW - 10 + i))
        .collect();

    let results = join_all(versions.iter().map(|event| save(&core, &relay.id, event))).await;
    results.into_iter().collect::<Result<Vec<_>>>()?;

    assert_eq!(
        ids(&core, &relay.id, kinds(&[10002])).await?,
        vec![versions[9].id.to_bytes()]
    );
    Ok(())
}

#[tokio::test]
async fn a_deletion_removes_its_authors_events_and_refuses_them_after() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let author = SecretKey::generate();
    let stranger = SecretKey::generate();

    let mine = sign_event(&author, 1, "mine", &[], NOW - 10);
    let theirs = sign_event(&stranger, 1, "theirs", &[], NOW - 10);
    save(&core, &relay.id, &mine).await?;
    save(&core, &relay.id, &theirs).await?;

    let deletion = sign_event(
        &author,
        5,
        "",
        &[&["e", &mine.id.to_hex()], &["e", &theirs.id.to_hex()]],
        NOW,
    );
    assert_eq!(save(&core, &relay.id, &deletion).await?, Saved::New);

    assert_eq!(
        ids(&core, &relay.id, kinds(&[1])).await?,
        vec![theirs.id.to_bytes()],
        "only the deleter's own event goes"
    );
    assert_eq!(save(&core, &relay.id, &mine).await?, Saved::Deleted);
    assert_eq!(save(&core, &relay.id, &theirs).await?, Saved::Duplicate);
    Ok(())
}

#[tokio::test]
async fn deleting_an_address_removes_versions_up_to_the_deletion() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let author = SecretKey::generate();
    let pubkey = author.public_key().to_hex();

    let article = sign_event(&author, 30023, "v1", &[&["d", "post"]], NOW - 10);
    save(&core, &relay.id, &article).await?;

    let address = format!("30023:{pubkey}:post");
    save(
        &core,
        &relay.id,
        &sign_event(&author, 5, "", &[&["a", &address]], NOW - 5),
    )
    .await?;
    assert!(ids(&core, &relay.id, kinds(&[30023])).await?.is_empty());

    // An older version stays deleted; a newer one is a new post.
    let older = sign_event(&author, 30023, "v0", &[&["d", "post"]], NOW - 8);
    assert_eq!(save(&core, &relay.id, &older).await?, Saved::Deleted);
    let newer = sign_event(&author, 30023, "v2", &[&["d", "post"]], NOW);
    assert_eq!(save(&core, &relay.id, &newer).await?, Saved::New);
    Ok(())
}

#[tokio::test]
async fn vanishing_removes_an_author_and_their_gift_wraps() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let author = SecretKey::generate();
    let friend = SecretKey::generate();
    let pubkey = author.public_key().to_hex();

    let note = sign_event(&author, 1, "bye", &[], NOW - 10);
    let wrap = sign_event(&friend, 1059, "sealed", &[&["p", &pubkey]], NOW - 10);
    let unrelated = sign_event(&friend, 1, "stays", &[], NOW - 10);
    for event in [&note, &wrap, &unrelated] {
        save(&core, &relay.id, event).await?;
    }

    let vanish = sign_event(&author, 62, "", &[&["relay", "ALL_RELAYS"]], NOW - 5);
    save(&core, &relay.id, &vanish).await?;

    assert_eq!(
        ids(&core, &relay.id, kinds(&[1, 1059])).await?,
        vec![unrelated.id.to_bytes()]
    );
    assert_eq!(save(&core, &relay.id, &note).await?, Saved::Deleted);
    assert_eq!(
        save(&core, &relay.id, &sign_event(&author, 1, "back", &[], NOW)).await?,
        Saved::New
    );
    Ok(())
}

#[tokio::test]
async fn expired_events_are_hidden_then_swept() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let author = SecretKey::generate();
    let expiring = sign_event(
        &author,
        1,
        "soon gone",
        &[&["expiration", &(NOW + 60).to_string()]],
        NOW,
    );
    save(&core, &relay.id, &expiring).await?;

    assert_eq!(ids(&core, &relay.id, Filter::new()).await?.len(), 1);

    let later =
        events::query_events(core.db.pool(), &relay.id, &kinds(&[1]), 1000, NOW + 60).await?;
    assert!(later.is_empty());

    events::delete_expired_events(core.db.pool(), NOW + 60, 1000).await?;
    assert!(ids(&core, &relay.id, Filter::new()).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn events_with_a_nul_character_are_not_storable() {
    let author = SecretKey::generate();
    assert!(!events::is_storable(&sign_event(
        &author,
        1,
        "a\0b",
        &[],
        NOW
    )));
    assert!(!events::is_storable(&sign_event(
        &author,
        1,
        "ok",
        &[&["t", "a\0"]],
        NOW
    )));
    assert!(events::is_storable(&sign_event(&author, 1, "ok", &[], NOW)));
}

#[tokio::test]
async fn a_stripped_event_reads_back_with_a_zero_signature() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let event = sign_event(&SecretKey::generate(), 1, "signed", &[], NOW);
    save(&core, &relay.id, &event).await?;

    let stored = &query(&core, &relay.id, Filter::new()).await?[0];
    let parsed: serde_json::Value = serde_json::from_str(&stored.to_json(true))?;
    assert_eq!(parsed["sig"], "0".repeat(128));
    Ok(())
}

#[tokio::test]
async fn negentropy_heads_refuse_sets_over_the_cap() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let author = SecretKey::generate();
    for i in 0..3 {
        save(
            &core,
            &relay.id,
            &sign_event(&author, 1, &i.to_string(), &[], NOW - i),
        )
        .await?;
    }

    let heads = events::query_event_heads(core.db.pool(), &relay.id, &kinds(&[1]), 3, NOW).await?;
    assert_eq!(heads.map(|heads| heads.len()), Some(3));
    let over = events::query_event_heads(core.db.pool(), &relay.id, &kinds(&[1]), 2, NOW).await?;
    assert!(over.is_none());
    Ok(())
}
