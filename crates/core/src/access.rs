//! Relay access: who belongs to a relay and with which roles, who may call which
//! NIP-86 methods, who and what is banned, and the claims that let people join.
//! Every change writes the relay-signed events that publish it in the same
//! transaction, and returns them for the caller to deliver once it commits.

use coracle_lib::events::Event;
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgExecutor};

use crate::crypto::RelayKey;
use crate::error::{Error, Result};
use crate::events::{self, DELETION};
use crate::relay_policy::kinds::{
    RELAY_ADD_MEMBER, RELAY_MEMBERS, RELAY_REMOVE_MEMBER, RELAY_ROLE,
};

/// Every NIP-86 method a relay's managers may be granted.
pub const MANAGED_METHODS: [&str; 23] = [
    "changerelayname",
    "changerelaydescription",
    "changerelayicon",
    "banpubkey",
    "unbanpubkey",
    "allowpubkey",
    "unallowpubkey",
    "listbannedpubkeys",
    "listallowedpubkeys",
    "banevent",
    "allowevent",
    "listbannedevents",
    "createrole",
    "editrole",
    "deleterole",
    "assignrole",
    "unassignrole",
    "assignmethod",
    "unassignmethod",
    "listmethodassignees",
    "listclaims",
    "createclaim",
    "deleteclaim",
];

/// The methods every member holds on a relay whose members may invite.
pub const INVITE_METHODS: [&str; 2] = ["listclaims", "createclaim"];

/// How a pubkey became a member.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum MemberSource {
    /// The relay's tenant, who is always a member.
    Owner,
    /// Added by the tenant, an admin, or a manager.
    Manual,
    /// Joined with an invite claim.
    Claim,
    /// Joined a relay that takes anyone.
    PublicJoin,
    /// Paid for a membership.
    Payment,
    /// Carried over from zooid.
    Migrated,
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct RelayMember {
    pub pubkey: String,
    pub source: MemberSource,
    /// The ids of the roles the member holds, in the order they were assigned.
    pub roles: Vec<String>,
    pub created_at: i64,
}

/// A label a relay gives some of its members, shown by clients.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct Role {
    pub id: String,
    pub label: String,
    pub description: String,
    /// A color hue from 0 to 360, or 0 for the client's default.
    pub hue: i32,
    /// Where the role sorts among the relay's roles, or 0 for the client's default.
    pub position: i32,
}

/// A banned pubkey or event id, and why.
#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Ban {
    pub subject: String,
    pub reason: String,
    pub created_at: i64,
}

/// A claim's history: when it was created, whether it was revoked, and every join
/// made with it, oldest first. It outlives the claim's revocation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaimLog {
    pub claim: String,
    pub created_at: i64,
    pub revoked_at: Option<i64>,
    pub uses: Vec<ClaimUse>,
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct ClaimUse {
    pub pubkey: String,
    pub created_at: i64,
}

// --- Reads ---

pub async fn list_members(db: impl PgExecutor<'_>, relay_id: &str) -> Result<Vec<RelayMember>> {
    Ok(sqlx::query_as::<_, RelayMember>(
        "SELECT pubkey, source, roles, created_at FROM relay_member
         WHERE relay_id = $1 ORDER BY created_at, pubkey",
    )
    .bind(relay_id)
    .fetch_all(db)
    .await?)
}

pub async fn get_member(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    pubkey: &str,
) -> Result<Option<RelayMember>> {
    Ok(sqlx::query_as::<_, RelayMember>(
        "SELECT pubkey, source, roles, created_at FROM relay_member WHERE relay_id = $1 AND pubkey = $2",
    )
    .bind(relay_id)
    .bind(pubkey)
    .fetch_optional(db)
    .await?)
}

pub async fn list_roles(db: impl PgExecutor<'_>, relay_id: &str) -> Result<Vec<Role>> {
    Ok(sqlx::query_as::<_, Role>(
        "SELECT id, label, description, hue, position FROM role WHERE relay_id = $1 ORDER BY position, id",
    )
    .bind(relay_id)
    .fetch_all(db)
    .await?)
}

/// Every pubkey with methods granted individually, and its methods.
pub async fn list_method_grants(
    db: impl PgExecutor<'_>,
    relay_id: &str,
) -> Result<Vec<(String, Vec<String>)>> {
    Ok(sqlx::query_as::<_, (String, Vec<String>)>(
        "SELECT pubkey, array_agg(method ORDER BY created_at, method) FROM method_grant
         WHERE relay_id = $1 GROUP BY pubkey ORDER BY pubkey",
    )
    .bind(relay_id)
    .fetch_all(db)
    .await?)
}

pub async fn list_banned_pubkeys(db: impl PgExecutor<'_>, relay_id: &str) -> Result<Vec<Ban>> {
    Ok(sqlx::query_as::<_, Ban>(
        "SELECT pubkey AS subject, reason, created_at FROM banned_pubkey
         WHERE relay_id = $1 ORDER BY created_at, pubkey",
    )
    .bind(relay_id)
    .fetch_all(db)
    .await?)
}

pub async fn list_banned_events(db: impl PgExecutor<'_>, relay_id: &str) -> Result<Vec<Ban>> {
    Ok(sqlx::query_as::<_, Ban>(
        "SELECT event_id AS subject, reason, created_at FROM banned_event
         WHERE relay_id = $1 ORDER BY created_at, event_id",
    )
    .bind(relay_id)
    .fetch_all(db)
    .await?)
}

/// The relay's claims that haven't been revoked.
pub async fn list_claims(db: impl PgExecutor<'_>, relay_id: &str) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT code FROM claim WHERE relay_id = $1 AND revoked_at IS NULL ORDER BY created_at, code",
    )
    .bind(relay_id)
    .fetch_all(db)
    .await?)
}

/// Whether a claim exists and hasn't been revoked.
pub async fn is_live_claim(db: impl PgExecutor<'_>, relay_id: &str, code: &str) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM claim WHERE relay_id = $1 AND code = $2 AND revoked_at IS NULL)",
    )
    .bind(relay_id)
    .bind(code)
    .fetch_one(db)
    .await?)
}

/// A claim's history, or `None` for a claim the relay never had.
pub async fn get_claim_log(
    db: impl PgExecutor<'_> + Copy,
    relay_id: &str,
    code: &str,
) -> Result<Option<ClaimLog>> {
    let Some((created_at, revoked_at)) = sqlx::query_as::<_, (i64, Option<i64>)>(
        "SELECT created_at, revoked_at FROM claim WHERE relay_id = $1 AND code = $2",
    )
    .bind(relay_id)
    .bind(code)
    .fetch_optional(db)
    .await?
    else {
        return Ok(None);
    };

    let uses = sqlx::query_as::<_, ClaimUse>(
        "SELECT pubkey, created_at FROM claim_use
         WHERE relay_id = $1 AND claim_code = $2 ORDER BY created_at",
    )
    .bind(relay_id)
    .bind(code)
    .fetch_all(db)
    .await?;

    Ok(Some(ClaimLog {
        claim: code.to_string(),
        created_at,
        revoked_at,
        uses,
    }))
}

// --- Members ---

/// Make a pubkey a member, publishing its addition and the new member list. A
/// pubkey that is already a member keeps how it joined, and nothing is written.
pub async fn admit(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    pubkey: &str,
    source: MemberSource,
    now: i64,
) -> Result<Vec<Event>> {
    let inserted = sqlx::query(
        "INSERT INTO relay_member (relay_id, pubkey, source, created_at) VALUES ($1, $2, $3, $4)
         ON CONFLICT (relay_id, pubkey) DO NOTHING",
    )
    .bind(relay_id)
    .bind(pubkey)
    .bind(source)
    .bind(now)
    .execute(&mut *conn)
    .await?
    .rows_affected();

    if inserted == 0 {
        return Ok(Vec::new());
    }

    let added = key.sign(
        RELAY_ADD_MEMBER,
        vec![tag(&["-"]), tag(&["p", pubkey])],
        "",
        now,
    );
    store(&mut *conn, relay_id, &added, now).await?;
    let list = publish_member_list(conn, key, relay_id, now).await?;
    Ok(vec![added, list])
}

/// Admit a new member only if the relay has room for one under `limit`, counting
/// under a lock on the relay so racing joins can't overshoot it.
pub async fn admit_within_limit(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    pubkey: &str,
    source: MemberSource,
    limit: Option<usize>,
    now: i64,
) -> Result<Vec<Event>> {
    sqlx::query("SELECT 1 FROM relay WHERE id = $1 FOR UPDATE")
        .bind(relay_id)
        .execute(&mut *conn)
        .await?;

    if let Some(limit) = limit
        && get_member(&mut *conn, relay_id, pubkey).await?.is_none()
        && count_members(&mut *conn, relay_id).await? >= limit as i64
    {
        return Err(Error::conflict(
            "member-limit",
            "this relay has reached its member limit",
        ));
    }

    admit(conn, key, relay_id, pubkey, source, now).await
}

/// Remove a member, with the methods granted to them, publishing the removal and
/// the new member list. The relay's owner and its own key can't be removed.
pub async fn remove(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    owner: &str,
    pubkey: &str,
    now: i64,
) -> Result<Vec<Event>> {
    if pubkey == owner || pubkey == key.pubkey() {
        return Err(Error::conflict(
            "owner",
            "Can't remove the relay owner from the relay.",
        ));
    }

    let removed = sqlx::query("DELETE FROM relay_member WHERE relay_id = $1 AND pubkey = $2")
        .bind(relay_id)
        .bind(pubkey)
        .execute(&mut *conn)
        .await?
        .rows_affected();

    if removed == 0 {
        return Ok(Vec::new());
    }

    let event = key.sign(
        RELAY_REMOVE_MEMBER,
        vec![tag(&["-"]), tag(&["p", pubkey])],
        "",
        now,
    );
    store(&mut *conn, relay_id, &event, now).await?;
    let list = publish_member_list(conn, key, relay_id, now).await?;
    Ok(vec![event, list])
}

pub async fn count_members(db: impl PgExecutor<'_>, relay_id: &str) -> Result<i64> {
    Ok(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM relay_member WHERE relay_id = $1")
            .bind(relay_id)
            .fetch_one(db)
            .await?,
    )
}

/// Ban a pubkey: remove it as a member, record the ban, and delete everything it
/// wrote on the relay.
pub async fn ban_pubkey(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    owner: &str,
    pubkey: &str,
    reason: &str,
    now: i64,
) -> Result<Vec<Event>> {
    let published = remove(&mut *conn, key, relay_id, owner, pubkey, now).await?;

    sqlx::query(
        "INSERT INTO banned_pubkey (relay_id, pubkey, reason, created_at) VALUES ($1, $2, $3, $4)
         ON CONFLICT (relay_id, pubkey) DO NOTHING",
    )
    .bind(relay_id)
    .bind(pubkey)
    .bind(reason)
    .bind(now)
    .execute(&mut *conn)
    .await?;

    let pubkey_bytes =
        hex::decode(pubkey).map_err(|_| Error::invalid("invalid-pubkey", "invalid pubkey"))?;
    sqlx::query("DELETE FROM event WHERE relay_id = $1 AND pubkey = $2")
        .bind(relay_id)
        .bind(pubkey_bytes)
        .execute(&mut *conn)
        .await?;

    Ok(published)
}

pub async fn unban_pubkey(db: impl PgExecutor<'_>, relay_id: &str, pubkey: &str) -> Result<()> {
    sqlx::query("DELETE FROM banned_pubkey WHERE relay_id = $1 AND pubkey = $2")
        .bind(relay_id)
        .bind(pubkey)
        .execute(db)
        .await?;
    Ok(())
}

/// Let a pubkey in: make it a member and lift any ban on it.
pub async fn allow_pubkey(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    pubkey: &str,
    now: i64,
) -> Result<Vec<Event>> {
    let published = admit(&mut *conn, key, relay_id, pubkey, MemberSource::Manual, now).await?;
    unban_pubkey(conn, relay_id, pubkey).await?;
    Ok(published)
}

/// Ban an event: delete it and refuse it from then on.
pub async fn ban_event(
    conn: &mut PgConnection,
    relay_id: &str,
    event_id: &str,
    reason: &str,
    now: i64,
) -> Result<()> {
    let id = hex::decode(event_id).map_err(|_| Error::invalid("invalid-id", "invalid event id"))?;
    sqlx::query("DELETE FROM event WHERE relay_id = $1 AND id = $2")
        .bind(relay_id)
        .bind(id)
        .execute(&mut *conn)
        .await?;

    sqlx::query(
        "INSERT INTO banned_event (relay_id, event_id, reason, created_at) VALUES ($1, $2, $3, $4)
         ON CONFLICT (relay_id, event_id) DO UPDATE SET reason = EXCLUDED.reason",
    )
    .bind(relay_id)
    .bind(event_id)
    .bind(reason)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn allow_event(db: impl PgExecutor<'_>, relay_id: &str, event_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM banned_event WHERE relay_id = $1 AND event_id = $2")
        .bind(relay_id)
        .bind(event_id)
        .execute(db)
        .await?;
    Ok(())
}

// --- Roles ---

/// Define a role, publishing it.
pub async fn create_role(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    role: &Role,
    now: i64,
) -> Result<Vec<Event>> {
    validate_role(role)?;

    let inserted = sqlx::query(
        "INSERT INTO role (relay_id, id, label, description, hue, position, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (relay_id, id) DO NOTHING",
    )
    .bind(relay_id)
    .bind(&role.id)
    .bind(&role.label)
    .bind(&role.description)
    .bind(role.hue)
    .bind(role.position)
    .bind(now)
    .execute(&mut *conn)
    .await?
    .rows_affected();

    if inserted == 0 {
        return Err(Error::conflict(
            "role-exists",
            format!("role {:?} already exists", role.id),
        ));
    }

    Ok(vec![publish_role(conn, key, relay_id, role, now).await?])
}

/// Redefine a role, publishing it.
pub async fn edit_role(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    role: &Role,
    now: i64,
) -> Result<Vec<Event>> {
    validate_role(role)?;

    let updated = sqlx::query(
        "UPDATE role SET label = $3, description = $4, hue = $5, position = $6 WHERE relay_id = $1 AND id = $2",
    )
    .bind(relay_id)
    .bind(&role.id)
    .bind(&role.label)
    .bind(&role.description)
    .bind(role.hue)
    .bind(role.position)
    .execute(&mut *conn)
    .await?
    .rows_affected();

    if updated == 0 {
        return Err(Error::conflict(
            "role-missing",
            format!("role {:?} does not exist", role.id),
        ));
    }

    Ok(vec![publish_role(conn, key, relay_id, role, now).await?])
}

/// Delete a role and take it from everyone who held it, publishing a deletion of its
/// definition and the new member list.
pub async fn delete_role(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    role_id: &str,
    now: i64,
) -> Result<Vec<Event>> {
    let deleted = sqlx::query("DELETE FROM role WHERE relay_id = $1 AND id = $2")
        .bind(relay_id)
        .bind(role_id)
        .execute(&mut *conn)
        .await?
        .rows_affected();

    let mut published = Vec::new();
    if deleted > 0 {
        let address = format!("{RELAY_ROLE}:{}:{role_id}", key.pubkey());
        let deletion = key.sign(
            DELETION,
            vec![tag(&["a", &address]), tag(&["k", &RELAY_ROLE.to_string()])],
            "",
            now,
        );
        store(&mut *conn, relay_id, &deletion, now).await?;
        events::apply_deletion(&mut *conn, relay_id, &deletion).await?;
        published.push(deletion);
    }

    let unassigned = sqlx::query(
        "UPDATE relay_member SET roles = array_remove(roles, $2) WHERE relay_id = $1 AND $2 = ANY(roles)",
    )
    .bind(relay_id)
    .bind(role_id)
    .execute(&mut *conn)
    .await?
    .rows_affected();

    if unassigned > 0 {
        published.push(publish_member_list(conn, key, relay_id, now).await?);
    }

    Ok(published)
}

/// Give a pubkey a role, making it a member first if it isn't one.
pub async fn assign_role(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    pubkey: &str,
    role_id: &str,
    now: i64,
) -> Result<Vec<Event>> {
    require_role(&mut *conn, relay_id, role_id).await?;
    let mut published = admit(&mut *conn, key, relay_id, pubkey, MemberSource::Manual, now).await?;

    let assigned = sqlx::query(
        "UPDATE relay_member SET roles = array_append(roles, $3)
         WHERE relay_id = $1 AND pubkey = $2 AND NOT ($3 = ANY(roles))",
    )
    .bind(relay_id)
    .bind(pubkey)
    .bind(role_id)
    .execute(&mut *conn)
    .await?
    .rows_affected();

    if assigned > 0 {
        published.retain(|event| event.kind != RELAY_MEMBERS);
        published.push(publish_member_list(conn, key, relay_id, now).await?);
    }

    Ok(published)
}

pub async fn unassign_role(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    pubkey: &str,
    role_id: &str,
    now: i64,
) -> Result<Vec<Event>> {
    let unassigned = sqlx::query(
        "UPDATE relay_member SET roles = array_remove(roles, $3)
         WHERE relay_id = $1 AND pubkey = $2 AND $3 = ANY(roles)",
    )
    .bind(relay_id)
    .bind(pubkey)
    .bind(role_id)
    .execute(&mut *conn)
    .await?
    .rows_affected();

    if unassigned == 0 {
        return Ok(Vec::new());
    }

    Ok(vec![publish_member_list(conn, key, relay_id, now).await?])
}

fn validate_role(role: &Role) -> Result<()> {
    if role.id.is_empty() {
        return Err(Error::invalid("role-id-required", "role id is required"));
    }
    if !(0..=360).contains(&role.hue) {
        return Err(Error::invalid(
            "invalid-hue",
            "color hue must be between 0 and 360",
        ));
    }
    Ok(())
}

async fn require_role(conn: &mut PgConnection, relay_id: &str, role_id: &str) -> Result<()> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM role WHERE relay_id = $1 AND id = $2)",
    )
    .bind(relay_id)
    .bind(role_id)
    .fetch_one(conn)
    .await?;

    if exists {
        Ok(())
    } else {
        Err(Error::conflict(
            "role-missing",
            format!("role {role_id:?} does not exist"),
        ))
    }
}

// --- Method grants ---

/// Grant a pubkey a NIP-86 method, making it a member first if it isn't one. The
/// owner and the relay's own key hold every method already.
pub async fn grant_method(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    owner: &str,
    pubkey: &str,
    method: &str,
    now: i64,
) -> Result<Vec<Event>> {
    if !MANAGED_METHODS.contains(&method) {
        return Err(Error::invalid(
            "unknown-method",
            format!("unknown method {method:?}"),
        ));
    }
    if pubkey == owner || pubkey == key.pubkey() {
        return Err(Error::conflict(
            "implicit-methods",
            "the relay owner and the relay's own key always have every management method",
        ));
    }

    let published = admit(&mut *conn, key, relay_id, pubkey, MemberSource::Manual, now).await?;
    sqlx::query(
        "INSERT INTO method_grant (relay_id, pubkey, method, created_at) VALUES ($1, $2, $3, $4)
         ON CONFLICT DO NOTHING",
    )
    .bind(relay_id)
    .bind(pubkey)
    .bind(method)
    .bind(now)
    .execute(conn)
    .await?;

    Ok(published)
}

pub async fn revoke_method(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    owner: &str,
    pubkey: &str,
    method: &str,
) -> Result<()> {
    if pubkey == owner {
        return Err(Error::conflict(
            "implicit-methods",
            "the relay owner and the relay's own key always have every management method",
        ));
    }

    sqlx::query("DELETE FROM method_grant WHERE relay_id = $1 AND pubkey = $2 AND method = $3")
        .bind(relay_id)
        .bind(pubkey)
        .bind(method)
        .execute(db)
        .await?;
    Ok(())
}

// --- Claims ---

/// Register a claim, or bring a revoked one back. Creating a live claim again
/// does nothing.
pub async fn create_claim(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    code: &str,
    now: i64,
) -> Result<()> {
    if code.is_empty() {
        return Err(Error::invalid("claim-required", "claim is required"));
    }

    sqlx::query(
        "INSERT INTO claim (relay_id, code, created_at) VALUES ($1, $2, $3)
         ON CONFLICT (relay_id, code) DO UPDATE SET revoked_at = NULL",
    )
    .bind(relay_id)
    .bind(code)
    .bind(now)
    .execute(db)
    .await?;
    Ok(())
}

/// Stop a claim working, keeping its history.
pub async fn revoke_claim(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    code: &str,
    now: i64,
) -> Result<()> {
    sqlx::query(
        "UPDATE claim SET revoked_at = $3 WHERE relay_id = $1 AND code = $2 AND revoked_at IS NULL",
    )
    .bind(relay_id)
    .bind(code)
    .bind(now)
    .execute(db)
    .await?;
    Ok(())
}

/// Log a join made with a claim the relay knows, whether or not the claim is what
/// let the pubkey in.
pub async fn record_claim_use(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    code: &str,
    pubkey: &str,
    now: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO claim_use (relay_id, claim_code, pubkey, created_at)
         SELECT $1, $2, $3, $4 WHERE EXISTS (SELECT 1 FROM claim WHERE relay_id = $1 AND code = $2)",
    )
    .bind(relay_id)
    .bind(code)
    .bind(pubkey)
    .bind(now)
    .execute(db)
    .await?;
    Ok(())
}

// --- Published state ---

/// Rewrite the relay's member list (kind 13534): every member and their roles.
pub async fn publish_member_list(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    now: i64,
) -> Result<Event> {
    let mut tags = vec![tag(&["-"])];
    for member in list_members(&mut *conn, relay_id).await? {
        let mut member_tag = vec!["member".to_string(), member.pubkey];
        member_tag.extend(member.roles);
        tags.push(member_tag);
    }

    let event = key.sign(RELAY_MEMBERS, tags, "", now);
    store(conn, relay_id, &event, now).await?;
    Ok(event)
}

/// Rewrite a role's definition (kind 33534).
async fn publish_role(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    role: &Role,
    now: i64,
) -> Result<Event> {
    let event = key.sign(RELAY_ROLE, role_tags(role), "", now);
    store(conn, relay_id, &event, now).await?;
    Ok(event)
}

/// A role's definition as tags. A zero hue or position is left out, for the client
/// to fall back on its own default.
pub fn role_tags(role: &Role) -> Vec<Vec<String>> {
    let mut tags = vec![tag(&["-"]), tag(&["d", &role.id])];
    if !role.label.is_empty() {
        tags.push(tag(&["label", &role.label]));
    }
    if !role.description.is_empty() {
        tags.push(tag(&["description", &role.description]));
    }
    if role.hue != 0 {
        tags.push(tag(&["color", &role.hue.to_string()]));
    }
    if role.position != 0 {
        tags.push(tag(&["order", &role.position.to_string()]));
    }
    tags
}

/// Store an event the relay signed.
pub(crate) async fn store(
    conn: &mut PgConnection,
    relay_id: &str,
    event: &Event,
    now: i64,
) -> Result<()> {
    events::save_relay_event(conn, relay_id, event, now).await?;
    Ok(())
}

pub(crate) fn tag(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}
