//! Decoding a NIP-86 call from its method and params.

use serde_json::Value;

use crate::access::Role;
use crate::management::Call;
use caravel_util::encoding::is_hex64;
use caravel_util::error::{Error, Result};

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
            if is_hex64(&value) {
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
