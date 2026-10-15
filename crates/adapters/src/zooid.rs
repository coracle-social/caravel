//! Zooid's management API, which serves caravel's relays until the relay runtime
//! moves into this binary. Every call is signed with NIP-98 by the robot.

use std::pin::Pin;
use std::time::Duration;

use anyhow::{Result, bail};
use base64::Engine;
use bytes::Bytes;
use futures_util::{Stream, StreamExt, TryStream};
use nostr_sdk::prelude::*;
use reqwest::{Method, StatusCode};

/// A stream of bytes from zooid, handed back unread.
pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>;

#[derive(Clone)]
pub struct Zooid {
    url: String,
    keys: Keys,
}

/// One relay's metered usage as zooid reports it: blob storage now, and a
/// lifetime clock of time spent in LiveKit rooms.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct RelayUsage {
    pub storage_bytes: i64,
    pub participant_seconds: i64,
}

/// One reviewable item from a relay: a published event or an uploaded blob,
/// with the relay it came from and when that relay first stored it.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ModerationItem {
    #[serde(rename = "type")]
    pub item_type: String,
    pub tenant: String,
    pub seen_at: i64,
    pub data: serde_json::Value,
}

/// One page of the moderation feed, and whether its window held more.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ModerationPage {
    pub items: Vec<ModerationItem>,
    pub truncated: bool,
}

/// One line of an import stream zooid could not store.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ImportProblem {
    #[serde(default)]
    pub line: i64,
    #[serde(default)]
    pub event_id: String,
    pub message: String,
}

/// What an import did.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ImportResult {
    pub imported: i64,
    pub invalid: i64,
    #[serde(default)]
    pub problems: Vec<ImportProblem>,
}

impl Zooid {
    pub fn new(url: &str, secret: &str) -> Result<Self> {
        Ok(Self {
            url: url.trim_end_matches('/').to_string(),
            keys: Keys::parse(secret)?,
        })
    }

    /// Write a relay's whole config, creating the relay if zooid lacks it.
    pub async fn put_relay(&self, relay_id: &str, config: &serde_json::Value) -> Result<()> {
        let path = format!("relay/{relay_id}");
        let response = self.send(Method::PUT, &path, Some(config)).await?;

        if response.status() == StatusCode::NOT_FOUND {
            let response = self.send(Method::POST, &path, Some(config)).await?;
            return check(response, "POST", &path).await.map(drop);
        }

        check(response, "PUT", &path).await.map(drop)
    }

    /// Delete a relay's data and config. A relay zooid doesn't have is already gone.
    pub async fn delete_relay(&self, relay_id: &str) -> Result<()> {
        let path = format!("relay/{relay_id}");
        let response = self.send(Method::DELETE, &path, None).await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(());
        }

        check(response, "DELETE", &path).await.map(drop)
    }

    pub async fn list_members(&self, relay_id: &str) -> Result<Vec<String>> {
        #[derive(serde::Deserialize)]
        struct Members {
            members: Vec<String>,
        }

        let path = format!("relay/{relay_id}/members");
        let response = check(self.send(Method::GET, &path, None).await?, "GET", &path).await?;
        Ok(response.json::<Members>().await?.members)
    }

    pub async fn get_usage(&self, relay_id: &str) -> Result<RelayUsage> {
        let path = format!("relay/{relay_id}/usage");
        let response = check(self.send(Method::GET, &path, None).await?, "GET", &path).await?;
        Ok(response.json().await?)
    }

    /// The events and uploads seen across every relay within a window of `seen_at`.
    pub async fn list_moderation(
        &self,
        since: Option<i64>,
        until: Option<i64>,
    ) -> Result<ModerationPage> {
        let bounds: Vec<String> = [("since", since), ("until", until)]
            .into_iter()
            .filter_map(|(key, value)| value.map(|value| format!("{key}={value}")))
            .collect();
        let path = if bounds.is_empty() {
            "moderation".to_string()
        } else {
            format!("moderation?{}", bounds.join("&"))
        };

        let response = check(self.send(Method::GET, &path, None).await?, "GET", &path).await?;
        Ok(response.json().await?)
    }

    /// A stored blob's content type and bytes, or `None` if zooid has no such blob.
    pub async fn get_blob(
        &self,
        relay_id: &str,
        sha256: &str,
    ) -> Result<Option<(String, Vec<u8>)>> {
        let path = format!("relay/{relay_id}/blob/{sha256}");
        let response = self.send(Method::GET, &path, None).await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        let response = check(response, "GET", &path).await?;
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();

        Ok(Some((content_type, response.bytes().await?.to_vec())))
    }

    /// A relay's stored events as JSONL, or `None` if zooid doesn't host it.
    pub async fn export_events(&self, relay_id: &str) -> Result<Option<ByteStream>> {
        let path = format!("relay/{relay_id}/export");
        let request = self.request(streaming_client()?, &Method::GET, &path)?;
        let response = request.send().await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        let response = check(response, "GET", &path).await?;
        Ok(Some(response.bytes_stream().boxed()))
    }

    /// Stream JSONL events into a relay, or `None` if zooid doesn't host it.
    pub async fn import_events<S>(&self, relay_id: &str, body: S) -> Result<Option<ImportResult>>
    where
        S: TryStream + Send + 'static,
        S::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
        Bytes: From<S::Ok>,
    {
        let path = format!("relay/{relay_id}/import");
        let response = self
            .request(streaming_client()?, &Method::POST, &path)?
            .header("Content-Type", "application/x-ndjson")
            .body(reqwest::Body::wrap_stream(body))
            .send()
            .await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        let response = check(response, "POST", &path).await?;
        Ok(Some(response.json().await?))
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<reqwest::Response> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;
        let mut request = self.request(client, &method, path)?;
        if let Some(body) = body {
            request = request.json(body);
        }

        Ok(request.send().await?)
    }

    fn request(
        &self,
        client: reqwest::Client,
        method: &Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder> {
        let url = format!("{}/{}", self.url, path.trim_start_matches('/'));
        // Zooid matches the `u` tag against the path alone, without the query.
        let auth_url = url.split('?').next().unwrap_or(&url);
        let event = EventBuilder::new(Kind::HttpAuth, "")
            .tags([
                Tag::parse(["u", auth_url])?,
                Tag::parse(["method", method.as_str()])?,
            ])
            .sign_with_keys(&self.keys)?;
        let auth = format!(
            "Nostr {}",
            base64::engine::general_purpose::STANDARD.encode(event.as_json())
        );

        Ok(client
            .request(method.clone(), &url)
            .header("Authorization", auth))
    }
}

/// A client with no overall deadline for the JSONL streams, whose bodies take as
/// long as a relay's events do. A read timeout still bounds a stalled connection.
fn streaming_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .read_timeout(Duration::from_secs(60))
        .build()?)
}

async fn check(response: reqwest::Response, method: &str, path: &str) -> Result<reqwest::Response> {
    if response.status().is_success() {
        return Ok(response);
    }

    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    bail!("zooid {method} {path} returned {status}: {text}")
}
