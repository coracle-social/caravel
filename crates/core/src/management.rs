//! Managing a relay's access and rooms together: carrying out NIP-86 calls, and
//! rewriting every event the relay publishes about its own state.

use coracle_lib::events::Event;
use serde_json::{Value, json};
use sqlx::PgConnection;

use crate::Core;
use crate::access::{self, MANAGED_METHODS, Role};
use crate::crypto::RelayKey;
use crate::error::{Error, Result};
use crate::relays::{self, Relay};
use crate::rooms;

/// A NIP-86 call, decoded from its method and params.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    ChangeRelayName(String),
    ChangeRelayDescription(String),
    ChangeRelayIcon(String),
    BanPubkey(String, String),
    UnbanPubkey(String),
    AllowPubkey(String),
    UnallowPubkey(String),
    ListBannedPubkeys,
    ListAllowedPubkeys,
    BanEvent(String, String),
    AllowEvent(String),
    ListBannedEvents,
    CreateRole(Role),
    EditRole(Role),
    DeleteRole(String),
    AssignRole(String, String),
    UnassignRole(String, String),
    AssignMethod(String, String),
    UnassignMethod(String, String),
    ListMethodAssignees,
    ListClaims,
    CreateClaim(String),
    DeleteClaim(String),
}

/// What a call returned, and the events the relay signed to publish what it changed.
pub struct Outcome {
    pub result: Value,
    pub published: Vec<Event>,
}

impl Call {
    /// Decode a call. `supportedmethods` isn't a call: it is answered from what the
    /// caller may do.
    pub fn decode(method: &str, params: &[Value]) -> Result<Self> {
        let string = |i: usize| {
            params
                .get(i)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        let required = |i: usize, what: &str| {
            params
                .get(i)
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| invalid_params(&format!("missing {what} param for '{method}'")))
        };
        let hex64 = |i: usize, what: &str| {
            let value = required(i, what)?;
            if value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit()) {
                Ok(value.to_lowercase())
            } else {
                Err(invalid_params(&format!(
                    "invalid {what} param for '{method}'"
                )))
            }
        };
        let integer = |i: usize| {
            params
                .get(i)
                .and_then(Value::as_f64)
                .filter(|n| n.fract() == 0.0)
        };
        let role = || -> Result<Role> {
            if params.len() < 5 {
                return Err(invalid_params(&format!(
                    "invalid number of params for '{method}'"
                )));
            }
            let position = integer(4)
                .ok_or_else(|| invalid_params(&format!("invalid order param for '{method}'")))?;
            Ok(Role {
                id: string(0),
                label: string(1),
                description: string(2),
                hue: integer(3).unwrap_or_default() as i32,
                position: position as i32,
            })
        };

        Ok(match method {
            "changerelayname" => Self::ChangeRelayName(required(0, "name")?),
            "changerelaydescription" => Self::ChangeRelayDescription(required(0, "description")?),
            "changerelayicon" => Self::ChangeRelayIcon(required(0, "icon")?),
            "banpubkey" => Self::BanPubkey(hex64(0, "pubkey")?, string(1)),
            "unbanpubkey" => Self::UnbanPubkey(hex64(0, "pubkey")?),
            "allowpubkey" => Self::AllowPubkey(hex64(0, "pubkey")?),
            "unallowpubkey" => Self::UnallowPubkey(hex64(0, "pubkey")?),
            "listbannedpubkeys" => Self::ListBannedPubkeys,
            "listallowedpubkeys" => Self::ListAllowedPubkeys,
            "banevent" => Self::BanEvent(hex64(0, "id")?, string(1)),
            "allowevent" => Self::AllowEvent(hex64(0, "id")?),
            "listbannedevents" => Self::ListBannedEvents,
            "createrole" => Self::CreateRole(role()?),
            "editrole" => Self::EditRole(role()?),
            "deleterole" => Self::DeleteRole(required(0, "id")?),
            "assignrole" => Self::AssignRole(hex64(0, "pubkey")?, string(1)),
            "unassignrole" => Self::UnassignRole(hex64(0, "pubkey")?, string(1)),
            "assignmethod" => Self::AssignMethod(hex64(0, "pubkey")?, string(1)),
            "unassignmethod" => Self::UnassignMethod(hex64(0, "pubkey")?, string(1)),
            "listmethodassignees" => Self::ListMethodAssignees,
            "listclaims" => Self::ListClaims,
            "createclaim" => Self::CreateClaim(string(0)),
            "deleteclaim" => Self::DeleteClaim(string(0)),
            _ => {
                return Err(Error::conflict(
                    "unsupported",
                    format!("method {method} not supported"),
                ));
            }
        })
    }
}

fn invalid_params(reason: &str) -> Error {
    Error::invalid("invalid-params", format!("invalid params: {reason}"))
}

/// Carry out a NIP-86 call on a relay, whose caller may make it.
pub async fn call(core: &Core, relay: &Relay, call: Call) -> Result<Outcome> {
    let pool = core.db.pool();
    let relay_id = relay.id.as_str();
    let owner = relay.tenant_pubkey.as_str();
    let now = core.clock.now();

    let result = match &call {
        Call::ListBannedPubkeys => {
            let bans = access::list_banned_pubkeys(pool, relay_id).await?;
            return done(json!(
                bans.into_iter()
                    .map(|ban| json!({"pubkey": ban.subject, "reason": ban.reason}))
                    .collect::<Vec<_>>()
            ));
        }
        Call::ListAllowedPubkeys => {
            let members = access::list_members(pool, relay_id).await?;
            return done(json!(
                members
                    .into_iter()
                    .map(|member| json!({"pubkey": member.pubkey, "reason": "relay member"}))
                    .collect::<Vec<_>>()
            ));
        }
        Call::ListBannedEvents => {
            let bans = access::list_banned_events(pool, relay_id).await?;
            return done(json!(
                bans.into_iter()
                    .map(|ban| json!({"id": ban.subject, "reason": ban.reason}))
                    .collect::<Vec<_>>()
            ));
        }
        Call::ListMethodAssignees => {
            let all: Vec<&str> = MANAGED_METHODS.to_vec();
            let mut assignees = vec![
                json!({"pubkey": owner, "methods": all}),
                json!({"pubkey": relay.pubkey, "methods": all}),
            ];
            for (pubkey, methods) in access::list_method_grants(pool, relay_id).await? {
                if pubkey != owner && pubkey != relay.pubkey {
                    assignees.push(json!({"pubkey": pubkey, "methods": methods}));
                }
            }
            assignees.sort_by(|a, b| a["pubkey"].as_str().cmp(&b["pubkey"].as_str()));
            return done(json!(assignees));
        }
        Call::ListClaims => return done(json!(access::list_claims(pool, relay_id).await?)),
        Call::ChangeRelayName(name) => {
            relays::set_relay_info(pool, relay_id, Some(name), None, None).await?;
            json!(true)
        }
        Call::ChangeRelayDescription(description) => {
            relays::set_relay_info(pool, relay_id, None, Some(description), None).await?;
            json!(true)
        }
        Call::ChangeRelayIcon(icon) => {
            relays::set_relay_info(pool, relay_id, None, None, Some(icon)).await?;
            json!(true)
        }
        Call::UnbanPubkey(pubkey) => {
            access::unban_pubkey(pool, relay_id, pubkey).await?;
            json!(true)
        }
        Call::AllowEvent(id) => {
            access::allow_event(pool, relay_id, id).await?;
            json!(true)
        }
        Call::CreateClaim(code) => {
            access::create_claim(pool, relay_id, code, now).await?;
            json!(true)
        }
        Call::DeleteClaim(code) => {
            access::revoke_claim(pool, relay_id, code, now).await?;
            json!(true)
        }
        _ => {
            let key = RelayKey::decrypt(&core.config.platform_key, &relay.secret)?;
            let published = core
                .db
                .with_tx(async |conn| write(conn, &key, relay, &call, now).await)
                .await?;
            return Ok(Outcome {
                result: json!(true),
                published,
            });
        }
    };

    done(result)
}

fn done(result: Value) -> Result<Outcome> {
    Ok(Outcome {
        result,
        published: Vec::new(),
    })
}

/// The calls that change what the relay publishes about itself.
async fn write(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay: &Relay,
    call: &Call,
    now: i64,
) -> Result<Vec<Event>> {
    let relay_id = relay.id.as_str();
    let owner = relay.tenant_pubkey.as_str();

    match call {
        Call::BanPubkey(pubkey, reason) => {
            let mut published =
                access::ban_pubkey(&mut *conn, key, relay_id, owner, pubkey, reason, now).await?;
            published.extend(publish_room_admins(conn, key, relay_id, now).await?);
            Ok(published)
        }
        Call::AllowPubkey(pubkey) => access::allow_pubkey(conn, key, relay_id, pubkey, now).await,
        Call::UnallowPubkey(pubkey) => {
            let mut published =
                access::remove(&mut *conn, key, relay_id, owner, pubkey, now).await?;
            published.extend(publish_room_admins(conn, key, relay_id, now).await?);
            Ok(published)
        }
        Call::BanEvent(id, reason) => {
            access::ban_event(conn, relay_id, id, reason, now).await?;
            Ok(Vec::new())
        }
        Call::CreateRole(role) => access::create_role(conn, key, relay_id, role, now).await,
        Call::EditRole(role) => access::edit_role(conn, key, relay_id, role, now).await,
        Call::DeleteRole(id) => access::delete_role(conn, key, relay_id, id, now).await,
        Call::AssignRole(pubkey, role_id) => {
            access::assign_role(conn, key, relay_id, pubkey, role_id, now).await
        }
        Call::UnassignRole(pubkey, role_id) => {
            access::unassign_role(conn, key, relay_id, pubkey, role_id, now).await
        }
        Call::AssignMethod(pubkey, method) => {
            let mut published =
                access::grant_method(&mut *conn, key, relay_id, owner, pubkey, method, now).await?;
            published.extend(publish_room_admins(conn, key, relay_id, now).await?);
            Ok(published)
        }
        Call::UnassignMethod(pubkey, method) => {
            access::revoke_method(&mut *conn, relay_id, owner, pubkey, method).await?;
            publish_room_admins(conn, key, relay_id, now).await
        }
        _ => Ok(Vec::new()),
    }
}

/// Rewrite every room's admin list, since the relay's admins administer them all.
async fn publish_room_admins(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    now: i64,
) -> Result<Vec<Event>> {
    let mut published = Vec::new();
    for room in rooms::list_rooms(&mut *conn, relay_id).await? {
        published.push(rooms::publish_admins(&mut *conn, key, relay_id, &room.id, now).await?);
    }
    Ok(published)
}

/// Rewrite every event the relay publishes about its own state from the tables
/// that hold it: the member list, each role, and each room's metadata, admins,
/// members and pins.
pub async fn rebuild_derived(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    now: i64,
) -> Result<Vec<Event>> {
    let mut published = vec![access::publish_member_list(&mut *conn, key, relay_id, now).await?];

    for role in access::list_roles(&mut *conn, relay_id).await? {
        let event = key.sign(
            crate::relay_policy::kinds::RELAY_ROLE,
            access::role_tags(&role),
            "",
            now,
        );
        access::store(&mut *conn, relay_id, &event, now).await?;
        published.push(event);
    }

    for room in rooms::list_rooms(&mut *conn, relay_id).await? {
        published.push(rooms::publish_metadata(&mut *conn, key, relay_id, &room.id, now).await?);
        published.push(rooms::publish_admins(&mut *conn, key, relay_id, &room.id, now).await?);
        published.push(rooms::publish_members(&mut *conn, key, relay_id, &room.id, now).await?);
        published.push(rooms::publish_pins(&mut *conn, key, relay_id, &room.id, now).await?);
    }

    Ok(published)
}
