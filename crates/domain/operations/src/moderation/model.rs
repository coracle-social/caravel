//! The moderation feed: the events and uploads every moderated relay saw, for
//! admins to review.

use serde::Serialize;
use serde_json::Value;

use caravel_relays::media::Upload;

/// One reviewable item, with the relay it came from and when that relay first
/// stored it.
#[derive(Clone, Debug, Serialize)]
pub struct ModerationItem {
    pub relay_id: String,
    /// When the relay first stored this event, so the version that replaced a
    /// replaceable one is seen when it arrived.
    pub seen_at: i64,
    #[serde(flatten)]
    pub subject: Subject,
}

/// What is up for review.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "lowercase")]
pub enum Subject {
    /// An event, as NIP-01 JSON.
    Event(Value),
    Upload(Upload),
}

/// One page of the feed, and whether its window held more than fit.
#[derive(Clone, Debug, Serialize)]
pub struct ModerationPage {
    pub items: Vec<ModerationItem>,
    pub truncated: bool,
}
