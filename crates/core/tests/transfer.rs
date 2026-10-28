//! Moving events in and out of a relay as JSONL, ported from zooid's transfer
//! tests: round trips, idempotence, bad lines, and rebuilding rooms from the
//! history a dump carries.

use anyhow::Result;
use bytes::Bytes;
use caravel_billing::relays::Relay;
use caravel_core::testing::{create_relay, sign_event};
use caravel_core::{Core, testing, transfer};
use caravel_relays::events::StoredEvent;
use caravel_relays::transfer::ImportResult;
use caravel_util::error::Error;
use caravel_util::nostr::kinds::rooms as kinds;
use coracle_lib::events::Event;
use coracle_lib::filters::{Filter, TagMatch};
use coracle_lib::keys::SecretKey;
use futures_util::{StreamExt, TryStreamExt};

const NOW: i64 = 1_800_000_000;

async fn core() -> Core {
    let core = testing::core(&SecretKey::generate().public_key().to_hex()).await;
    core.clock.pin(NOW);
    core
}

fn line(event: &Event) -> String {
    serde_json::to_string(event).unwrap()
}

/// Import a dump, delivered in small chunks so lines straddle them.
async fn import(core: &Core, relay: &Relay, dump: &str) -> Result<ImportResult> {
    let chunks: Vec<std::result::Result<Bytes, std::io::Error>> = dump
        .as_bytes()
        .chunks(7)
        .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
        .collect();
    Ok(transfer::import_events(core, relay, futures_util::stream::iter(chunks)).await?)
}

async fn export(core: &Core, relay: &Relay) -> Result<String> {
    let pages: Vec<Bytes> = transfer::export_events(core.db.pool().clone(), relay.id.clone())
        .try_collect()
        .await?;
    Ok(pages
        .iter()
        .map(|page| String::from_utf8_lossy(page).to_string())
        .collect())
}

async fn stored(core: &Core, relay: &Relay, kind: u16, room_id: &str) -> Result<Vec<StoredEvent>> {
    let tag = if caravel_relays::rooms::is_metadata_kind(kind) {
        "d"
    } else {
        "h"
    };
    let filter = Filter {
        kinds: Some([kind].into()),
        ..Filter::new()
    }
    .add_tag(TagMatch::Any, tag, room_id);
    Ok(
        caravel_relays::events::queries::query_events(
            core.db.pool(),
            &relay.id,
            &filter,
            1000,
            NOW,
        )
        .await?,
    )
}

async fn is_room_member(core: &Core, relay: &Relay, room_id: &str, pubkey: &str) -> Result<bool> {
    Ok(
        caravel_relays::rooms::queries::list_room_members(core.db.pool(), &relay.id)
            .await?
            .iter()
            .any(|member| member.room_id == room_id && member.pubkey == pubkey),
    )
}

async fn room_name(core: &Core, relay: &Relay, room_id: &str) -> Result<Option<String>> {
    Ok(
        caravel_relays::rooms::queries::list_rooms(core.db.pool(), &relay.id)
            .await?
            .into_iter()
            .find(|room| room.id == room_id)
            .map(|room| {
                room.tags
                    .iter()
                    .find(|tag| tag.first().map(String::as_str) == Some("name"))
                    .and_then(|tag| tag.get(1).cloned())
                    .unwrap_or_default()
            }),
    )
}

#[tokio::test]
async fn an_export_imports_into_another_relay() -> Result<()> {
    let core = core().await;
    let (source, _) = create_relay(&core).await;
    let (target, _) = create_relay(&core).await;
    let author = SecretKey::generate();

    let first = sign_event(&author, 1, "first event", &[], NOW - 10);
    let second = sign_event(&author, 1, "second event", &[], NOW - 5);
    let dump = [line(&first), line(&second)].join("\n");
    assert_eq!(import(&core, &source, &dump).await?.imported, 2);

    let exported = export(&core, &source).await?;
    assert!(
        exported
            .lines()
            .any(|line| line.contains(&first.id.to_hex()))
    );
    assert!(
        exported
            .lines()
            .any(|line| line.contains(&second.id.to_hex()))
    );

    // The source's own membership events come along and are left to the target.
    let result = import(&core, &target, &exported).await?;
    assert_eq!(result.imported, 2, "{result:?}");
    assert_eq!(result.invalid, 0, "{result:?}");
    assert!(result.skipped > 0, "{result:?}");

    let ids: Vec<[u8; 32]> = caravel_relays::events::queries::get_events(
        core.db.pool(),
        &target.id,
        &[first.id.to_bytes(), second.id.to_bytes()],
    )
    .await?
    .iter()
    .map(|event| event.head.id)
    .collect();
    assert_eq!(ids.len(), 2);
    Ok(())
}

#[tokio::test]
async fn an_export_pages_through_every_event() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let author = SecretKey::generate();

    let mut dump = Vec::new();
    for i in 0..2500 {
        dump.push(line(&sign_event(
            &author,
            1,
            &i.to_string(),
            &[],
            NOW - 100 + i % 3,
        )));
    }
    assert_eq!(
        import(&core, &relay, &dump.join("\n")).await?.imported,
        2500
    );

    let exported = export(&core, &relay).await?;
    let mut authored = exported
        .lines()
        .filter(|line| line.contains(&author.public_key().to_hex()))
        .collect::<Vec<_>>();
    authored.sort();
    authored.dedup();
    assert_eq!(authored.len(), 2500);
    Ok(())
}

// Re-importing a dump changes nothing, so an operator can run the same file twice.
#[tokio::test]
async fn importing_a_dump_twice_is_harmless() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let event = sign_event(&SecretKey::generate(), 1, "only event", &[], NOW);

    for _ in 0..2 {
        let result = import(&core, &relay, &line(&event)).await?;
        assert_eq!((result.imported, result.invalid), (1, 0));
    }
    Ok(())
}

// A bad line is reported and the good ones still land.
#[tokio::test]
async fn an_import_skips_bad_lines_and_says_which() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let author = SecretKey::generate();

    let mut forged = serde_json::to_value(sign_event(&author, 1, "signed content", &[], NOW))?;
    forged["content"] = "tampered content".into();
    let unsigned =
        serde_json::json!({"kind": 1, "created_at": NOW, "content": "unsigned", "tags": []});
    let good = sign_event(&author, 1, "good event", &[], NOW);

    let dump = [
        String::new(),
        "not json at all".to_string(),
        forged.to_string(),
        unsigned.to_string(),
        line(&good),
    ]
    .join("\n");
    let result = import(&core, &relay, &dump).await?;

    assert_eq!(result.imported, 1, "{result:?}");
    assert_eq!(result.invalid, 3, "{result:?}");
    assert_eq!(result.problems[0].line, Some(2));
    assert_eq!(result.problems[1].line, Some(3));
    assert_eq!(result.problems[1].message, "invalid signature");
    assert_eq!(result.problems[2].line, Some(4));
    Ok(())
}

// Rooms are rebuilt from their history, and the state the source relay signed
// is dropped for this relay's own.
#[tokio::test]
async fn an_import_rebuilds_room_state() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let owner = SecretKey::generate();
    let member = SecretKey::generate().public_key().to_hex();
    let source = SecretKey::generate();

    let dump = [
        sign_event(
            &owner,
            kinds::CREATE_GROUP,
            "",
            &[&["h", "room"]],
            NOW - 300,
        ),
        sign_event(
            &owner,
            kinds::EDIT_METADATA,
            "",
            &[&["h", "room"], &["name", "imported room"]],
            NOW - 200,
        ),
        sign_event(
            &owner,
            kinds::PUT_USER,
            "",
            &[&["h", "room"], &["p", &member]],
            NOW - 100,
        ),
        sign_event(
            &source,
            kinds::METADATA,
            "",
            &[&["d", "room"], &["name", "source copy"]],
            NOW - 50,
        ),
    ]
    .iter()
    .map(line)
    .collect::<Vec<_>>()
    .join("\n");

    let result = import(&core, &relay, &dump).await?;
    assert_eq!(
        (result.imported, result.skipped, result.invalid),
        (3, 1, 0),
        "{result:?}"
    );
    assert_eq!(result.rooms_rebuilt, 1);

    let metadata = stored(&core, &relay, kinds::METADATA, "room").await?;
    assert_eq!(metadata.len(), 1);
    assert_eq!(metadata[0].head.pubkey_hex(), relay.pubkey);
    assert!(metadata[0].tags.contains("imported room"));

    let members = stored(&core, &relay, kinds::MEMBERS, "room").await?;
    assert_eq!(members.len(), 1);
    assert!(members[0].tags.contains(&member));
    Ok(())
}

// A join request replays the admission an arriving one would.
#[tokio::test]
async fn an_import_admits_join_requests() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let owner = SecretKey::generate();
    let joiner = SecretKey::generate();

    let dump = [
        sign_event(
            &owner,
            kinds::CREATE_GROUP,
            "",
            &[&["h", "room"]],
            NOW - 300,
        ),
        sign_event(
            &joiner,
            kinds::JOIN_REQUEST,
            "",
            &[&["h", "room"]],
            NOW - 200,
        ),
    ]
    .iter()
    .map(line)
    .collect::<Vec<_>>()
    .join("\n");

    assert_eq!(import(&core, &relay, &dump).await?.imported, 2);
    assert!(is_room_member(&core, &relay, "room", &joiner.public_key().to_hex()).await?);
    Ok(())
}

// A room is rebuilt from what the import stored, so a line it refused changes nothing.
#[tokio::test]
async fn an_import_replays_only_the_history_it_stored() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let owner = SecretKey::generate();
    let joiner = SecretKey::generate();

    let join = sign_event(
        &joiner,
        kinds::JOIN_REQUEST,
        "",
        &[&["h", "room"]],
        NOW - 200,
    );
    let dump = [
        sign_event(
            &owner,
            kinds::CREATE_GROUP,
            "",
            &[&["h", "room"]],
            NOW - 300,
        ),
        sign_event(&joiner, 5, "", &[&["e", &join.id.to_hex()]], NOW - 100),
        join,
    ]
    .iter()
    .map(line)
    .collect::<Vec<_>>()
    .join("\n");

    let result = import(&core, &relay, &dump).await?;
    assert_eq!(
        (result.imported, result.invalid, result.rooms_rebuilt),
        (2, 1, 1),
        "{result:?}"
    );
    assert!(!is_room_member(&core, &relay, "room", &joiner.public_key().to_hex()).await?);
    Ok(())
}

// A dump covering part of a room the relay runs adds to its state.
#[tokio::test]
async fn an_import_adds_to_an_existing_room() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let owner = SecretKey::generate();
    let joiner = SecretKey::generate();

    let create = sign_event(
        &owner,
        kinds::CREATE_GROUP,
        "",
        &[&["h", "room"], &["name", "already here"]],
        NOW - 300,
    );
    assert_eq!(
        import(&core, &relay, &line(&create)).await?.rooms_rebuilt,
        1
    );

    let join = sign_event(
        &joiner,
        kinds::JOIN_REQUEST,
        "",
        &[&["h", "room"]],
        NOW - 100,
    );
    assert_eq!(import(&core, &relay, &line(&join)).await?.imported, 1);

    assert!(is_room_member(&core, &relay, "room", &joiner.public_key().to_hex()).await?);
    assert_eq!(
        stored(&core, &relay, kinds::CREATE_GROUP, "room")
            .await?
            .len(),
        1
    );
    assert_eq!(
        room_name(&core, &relay, "room").await?.as_deref(),
        Some("already here")
    );
    Ok(())
}

// A room whose history never created it stays absent, and the import says so.
#[tokio::test]
async fn an_import_leaves_an_uncreated_room_alone() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let author = SecretKey::generate();
    let source = SecretKey::generate();

    let dump = [
        sign_event(&author, 9, "hello", &[&["h", "room"]], NOW - 100),
        sign_event(
            &source,
            kinds::METADATA,
            "",
            &[&["d", "room"], &["name", "imported"]],
            NOW - 50,
        ),
    ]
    .iter()
    .map(line)
    .collect::<Vec<_>>()
    .join("\n");

    let result = import(&core, &relay, &dump).await?;
    assert_eq!(
        (result.imported, result.skipped, result.rooms_rebuilt),
        (1, 1, 0),
        "{result:?}"
    );
    assert_eq!(result.problems.len(), 1);
    assert!(result.problems[0].message.contains("room"));

    assert_eq!(room_name(&core, &relay, "room").await?, None);
    assert!(
        stored(&core, &relay, kinds::CREATE_GROUP, "room")
            .await?
            .is_empty()
    );
    assert_eq!(stored(&core, &relay, 9, "room").await?.len(), 1);
    Ok(())
}

// A history ending in a deletion brings nothing back.
#[tokio::test]
async fn an_import_ending_in_a_deletion_leaves_no_room() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let owner = SecretKey::generate();

    let dump = [
        sign_event(
            &owner,
            kinds::CREATE_GROUP,
            "",
            &[&["h", "room"]],
            NOW - 300,
        ),
        sign_event(&owner, 9, "hello", &[&["h", "room"]], NOW - 200),
        sign_event(
            &owner,
            kinds::DELETE_GROUP,
            "",
            &[&["h", "room"]],
            NOW - 100,
        ),
    ]
    .iter()
    .map(line)
    .collect::<Vec<_>>()
    .join("\n");

    assert_eq!(import(&core, &relay, &dump).await?.imported, 3);
    assert_eq!(room_name(&core, &relay, "room").await?, None);
    assert!(stored(&core, &relay, 9, "room").await?.is_empty());
    Ok(())
}

// A room deleted and created again keeps what came after the deletion.
#[tokio::test]
async fn an_import_keeps_a_room_created_again_after_its_deletion() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let owner = SecretKey::generate();

    let dump = [
        sign_event(
            &owner,
            kinds::CREATE_GROUP,
            "",
            &[&["h", "room"]],
            NOW - 500,
        ),
        sign_event(
            &owner,
            kinds::DELETE_GROUP,
            "",
            &[&["h", "room"]],
            NOW - 400,
        ),
        sign_event(
            &owner,
            kinds::CREATE_GROUP,
            "",
            &[&["h", "room"], &["name", "second life"]],
            NOW - 300,
        ),
        sign_event(&owner, 9, "hello", &[&["h", "room"]], NOW - 200),
    ]
    .iter()
    .map(line)
    .collect::<Vec<_>>()
    .join("\n");

    assert_eq!(import(&core, &relay, &dump).await?.imported, 4);
    assert_eq!(
        room_name(&core, &relay, "room").await?.as_deref(),
        Some("second life")
    );
    assert_eq!(stored(&core, &relay, 9, "room").await?.len(), 1);
    Ok(())
}

// A relay without rooms stores room events as plain events.
#[tokio::test]
async fn a_relay_without_rooms_rebuilds_none() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let settings = serde_json::from_value(serde_json::json!({"groups_enabled": 0}))?;
    let relay = caravel_core::hosting::update_relay(&core, relay, settings).await?;
    let owner = SecretKey::generate();

    let create = sign_event(
        &owner,
        kinds::CREATE_GROUP,
        "",
        &[&["h", "room"]],
        NOW - 300,
    );
    let result = import(&core, &relay, &line(&create)).await?;
    assert_eq!(
        (result.imported, result.rooms_rebuilt),
        (1, 0),
        "{result:?}"
    );
    assert_eq!(room_name(&core, &relay, "room").await?, None);
    Ok(())
}

// A line too long to be an event is reported, and the lines after it still land.
#[tokio::test]
async fn an_overlong_line_is_reported_and_skipped() -> Result<()> {
    let core = core().await;
    let (relay, _) = create_relay(&core).await;
    let good = sign_event(&SecretKey::generate(), 1, "after", &[], NOW);

    let dump = format!("{}\n{}", "x".repeat(5 * 1024 * 1024), line(&good));
    let stream = futures_util::stream::iter(
        dump.into_bytes()
            .chunks(64 * 1024)
            .map(|chunk| Ok::<_, Error>(Bytes::copy_from_slice(chunk)))
            .collect::<Vec<_>>(),
    )
    .boxed();
    let result = transfer::import_events(&core, &relay, stream).await?;

    assert_eq!((result.imported, result.invalid), (1, 1), "{result:?}");
    assert_eq!(result.problems[0].line, Some(1));
    Ok(())
}

// Posting in a room the relay holds changes nothing about the room.
#[tokio::test]
async fn an_import_that_only_posts_in_a_room_rebuilds_nothing() -> Result<()> {
    let core = core().await;
    let (relay, owner) = create_relay(&core).await;
    let author = SecretKey::generate();

    let create = sign_event(
        &owner,
        kinds::CREATE_GROUP,
        "",
        &[&["h", "room"], &["name", "lobby"]],
        NOW - 100,
    );
    assert_eq!(
        import(&core, &relay, &line(&create)).await?.rooms_rebuilt,
        1
    );

    let chat = sign_event(&author, 9, "hello", &[&["h", "room"]], NOW - 50);
    let result = import(&core, &relay, &line(&chat)).await?;
    assert_eq!(
        (result.imported, result.rooms_rebuilt),
        (1, 0),
        "{result:?}"
    );
    assert!(result.problems.is_empty(), "{result:?}");
    assert_eq!(
        room_name(&core, &relay, "room").await?.as_deref(),
        Some("lobby")
    );
    Ok(())
}

// zooid could hold room events with an empty or missing id, and state it signed for them.
#[tokio::test]
async fn an_import_refuses_group_events_naming_no_group() -> Result<()> {
    let core = core().await;
    let (relay, owner) = create_relay(&core).await;
    let source = SecretKey::generate();

    let dump = [
        sign_event(
            &owner,
            kinds::CREATE_GROUP,
            "",
            &[&["name", "x"]],
            NOW - 400,
        ),
        sign_event(&owner, kinds::EDIT_METADATA, "", &[&["h", ""]], NOW - 300),
        sign_event(&owner, 9, "hello", &[&["h", ""]], NOW - 200),
        sign_event(&source, kinds::ADMINS, "", &[&["d", ""]], NOW - 100),
        sign_event(&owner, kinds::CREATE_GROUP, "", &[&["h", "room"]], NOW - 50),
    ]
    .iter()
    .map(line)
    .collect::<Vec<_>>()
    .join("\n");

    let result = import(&core, &relay, &dump).await?;
    assert_eq!(
        (
            result.imported,
            result.invalid,
            result.skipped,
            result.rooms_rebuilt
        ),
        (1, 3, 1, 1),
        "{result:?}"
    );
    assert_eq!(
        result
            .problems
            .iter()
            .map(|problem| problem.line)
            .collect::<Vec<_>>(),
        [Some(1), Some(2), Some(3)]
    );
    let ids: Vec<String> = caravel_relays::rooms::queries::list_rooms(core.db.pool(), &relay.id)
        .await?
        .into_iter()
        .map(|room| room.id)
        .collect();
    assert_eq!(ids, ["room"]);
    Ok(())
}
