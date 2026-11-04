//! The LiveKit server calls run on: access tokens for participants, its room
//! service, and checking the webhooks it sends.

use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use base64::Engine;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};

/// How long a participant token is good for, LiveKit's own default.
const TOKEN_TTL: i64 = 6 * 60 * 60;

#[derive(Clone)]
pub struct Livekit {
    url: String,
    api_key: String,
    api_secret: String,
    http: reqwest::Client,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct VideoGrant {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    room_join: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    room_create: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    room_list: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    room_admin: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    room: String,
}

#[derive(Serialize, Deserialize)]
struct Claims {
    iss: String,
    #[serde(default)]
    sub: String,
    nbf: i64,
    exp: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    identity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    video: Option<VideoGrant>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    sha256: String,
}

/// A webhook LiveKit sent, as far as calls care: what happened, in which room,
/// to which participant, and when.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookEvent {
    pub event: String,
    pub room: Option<Room>,
    pub participant: Option<Participant>,
    #[serde(default, deserialize_with = "int64")]
    pub created_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct Room {
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Participant {
    #[serde(default)]
    pub sid: String,
    #[serde(default)]
    pub identity: String,
    #[serde(default, deserialize_with = "int64")]
    pub joined_at: i64,
}

/// Protobuf's JSON writes 64-bit integers as strings.
fn int64<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Number(i64),
        Text(String),
    }

    match Raw::deserialize(deserializer)? {
        Raw::Number(value) => Ok(value),
        Raw::Text(value) => value.parse().map_err(serde::de::Error::custom),
    }
}

impl Livekit {
    pub fn new(url: &str, api_key: &str, api_secret: &str) -> Result<Self> {
        Ok(Self {
            url: url.to_string(),
            api_key: api_key.to_string(),
            api_secret: api_secret.to_string(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()?,
        })
    }

    /// The url clients connect to.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// A token admitting `identity` to `room`.
    pub fn participant_token(&self, room: &str, identity: &str, now: i64) -> Result<String> {
        self.token(
            identity,
            VideoGrant {
                room_join: true,
                room: room.to_string(),
                ..Default::default()
            },
            now,
        )
    }

    fn token(&self, identity: &str, video: VideoGrant, now: i64) -> Result<String> {
        let claims = Claims {
            iss: self.api_key.clone(),
            sub: identity.to_string(),
            nbf: now,
            exp: now + TOKEN_TTL,
            identity: identity.to_string(),
            video: Some(video),
            sha256: String::new(),
        };
        Ok(jsonwebtoken::encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(self.api_secret.as_bytes()),
        )?)
    }

    /// Create a room, or find it already there.
    pub async fn ensure_room(&self, room: &str, now: i64) -> Result<()> {
        let grant = VideoGrant {
            room_create: true,
            room_list: true,
            room_admin: true,
            ..Default::default()
        };
        let response = self
            .room_service(
                "CreateRoom",
                grant,
                serde_json::json!({ "name": room }),
                now,
            )
            .await?;

        if response.status().is_success() || response.status() == reqwest::StatusCode::CONFLICT {
            Ok(())
        } else {
            bail!("failed to create room: {}", response.status())
        }
    }

    /// The identities of everyone in a room.
    pub async fn list_participants(&self, room: &str, now: i64) -> Result<Vec<String>> {
        #[derive(Deserialize)]
        struct Listed {
            #[serde(default)]
            participants: Vec<Participant>,
        }

        let grant = VideoGrant {
            room_admin: true,
            room: room.to_string(),
            ..Default::default()
        };
        let response = self
            .room_service(
                "ListParticipants",
                grant,
                serde_json::json!({ "room": room }),
                now,
            )
            .await?;
        if !response.status().is_success() {
            bail!("ListParticipants failed: {}", response.status());
        }

        Ok(response
            .json::<Listed>()
            .await?
            .participants
            .into_iter()
            .map(|participant| participant.identity)
            .collect())
    }

    async fn room_service(
        &self,
        method: &str,
        grant: VideoGrant,
        body: serde_json::Value,
        now: i64,
    ) -> Result<reqwest::Response> {
        let base = self
            .url
            .replacen("wss://", "https://", 1)
            .replacen("ws://", "http://", 1);
        let token = self.token("", grant, now)?;

        Ok(self
            .http
            .post(format!(
                "{}/twirp/livekit.RoomService/{method}",
                base.trim_end_matches('/')
            ))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?)
    }

    /// A webhook's event, if its `Authorization` token is signed with this server's
    /// secret and vouches for exactly this body.
    pub fn receive_webhook(&self, authorization: &str, body: &[u8]) -> Result<WebhookEvent> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[&self.api_key]);
        validation.validate_nbf = true;
        validation.required_spec_claims.clear();

        let token = authorization
            .strip_prefix("Bearer ")
            .unwrap_or(authorization);
        let claims = jsonwebtoken::decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.api_secret.as_bytes()),
            &validation,
        )
        .map_err(|error| anyhow!("invalid webhook token: {error}"))?
        .claims;

        let checksum = base64::engine::general_purpose::STANDARD.encode(Sha256::digest(body));
        if claims.sha256 != checksum {
            bail!("the webhook's checksum does not match its body");
        }

        Ok(serde_json::from_slice(body)?)
    }

    /// A webhook token for `body`, as LiveKit signs one. For tests, and so not
    /// part of what a deployment links.
    #[cfg(any(test, feature = "testing"))]
    pub fn sign_webhook(&self, body: &[u8], now: i64) -> Result<String> {
        let claims = Claims {
            iss: self.api_key.clone(),
            sub: String::new(),
            nbf: now,
            exp: now + 300,
            identity: String::new(),
            video: None,
            sha256: base64::engine::general_purpose::STANDARD.encode(Sha256::digest(body)),
        };
        Ok(jsonwebtoken::encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(self.api_secret.as_bytes()),
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_webhook_is_trusted_only_with_a_matching_signature_and_checksum() {
        let livekit = Livekit::new("wss://lk.test", "key", "secret").unwrap();
        let body = br#"{"event":"participant_left","room":{"name":"r:g"},"participant":{"sid":"PA","identity":"x","joinedAt":"100"},"createdAt":"160"}"#;
        let now = jsonwebtoken::get_current_timestamp() as i64;
        let token = livekit.sign_webhook(body, now).unwrap();

        let event = livekit.receive_webhook(&token, body).unwrap();
        assert_eq!(event.event, "participant_left");
        assert_eq!(event.participant.unwrap().joined_at, 100);
        assert_eq!(event.created_at, 160);

        assert!(livekit.receive_webhook(&token, b"{}").is_err());
        let other = Livekit::new("wss://lk.test", "key", "other").unwrap();
        assert!(other.receive_webhook(&token, body).is_err());
    }
}
