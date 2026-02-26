use anyhow::{anyhow, Result};
use base64::engine::general_purpose;
use base64::Engine;
use std::str::FromStr;

use nostr_sdk::nostr::key::PublicKey;
use nostr_sdk::nostr::nips::nip98::HttpMethod;
use nostr_sdk::nostr::types::url::Url;
use nostr_sdk::nostr::{Alphabet, Event, Kind, SingleLetterTag, TagKind, TagStandard};
use nostr_sdk::JsonUtil;

pub fn verify_nip98(auth_header: &str, url: &str, method: &str) -> Result<PublicKey> {
    let url = Url::parse(url)?;
    let method = HttpMethod::from_str(&method.to_uppercase())?;

    let event = decode_auth_event(auth_header)?;

    if event.kind != Kind::HttpAuth {
        return Err(anyhow!("authorization event kind mismatch"));
    }

    let authorized_url =
        match event
            .tags
            .find_standardized(TagKind::SingleLetter(SingleLetterTag::lowercase(
                Alphabet::U,
            ))) {
            Some(TagStandard::AbsoluteURL(url)) => url,
            _ => return Err(anyhow!("authorization header missing url tag")),
        };

    let authorized_method = match event.tags.find_standardized(TagKind::Method) {
        Some(TagStandard::Method(method)) => method,
        _ => return Err(anyhow!("authorization header missing method tag")),
    };

    if authorized_url != &url || authorized_method != &method {
        return Err(anyhow!("authorization does not match request"));
    }

    event.verify()?;
    Ok(event.pubkey)
}

fn decode_auth_event(auth_header: &str) -> Result<Event> {
    if auth_header.trim().is_empty() {
        return Err(anyhow!("missing authorization header"));
    }

    let (prefix, encoded) = auth_header
        .split_once(' ')
        .ok_or_else(|| anyhow!("malformed authorization header"))?;

    if prefix != "Nostr" || encoded.is_empty() {
        return Err(anyhow!("malformed authorization header"));
    }

    let decoded = general_purpose::STANDARD.decode(encoded)?;
    let json = String::from_utf8(decoded)?;
    Ok(Event::from_json(json)?)
}
