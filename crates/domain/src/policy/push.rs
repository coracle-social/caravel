//! The push subscriptions a relay accepts.

use coracle_lib::events::Event;

use crate::policy::Refusal;

pub const MAX_PUSH_SUBSCRIPTIONS: usize = 10;

pub fn check_push_subscription(
    event: &Event,
    hosts: &[String],
    held: usize,
) -> Result<(), Refusal> {
    if event.tags.value("d").is_none_or(str::is_empty) {
        return Err(Refusal::invalid("missing or empty d tag"));
    }

    let relay = event
        .tags
        .value("relay")
        .ok_or_else(|| Refusal::invalid("missing relay tag"))?;
    if !hosts.contains(&caravel_util::urls::url_host(relay)) {
        return Err(Refusal::invalid(
            "relay tag does not match this relay's URL",
        ));
    }

    let filters: Vec<&str> = event.tags.values("filter").collect();
    if filters.is_empty() {
        return Err(Refusal::invalid("at least one filter tag is required"));
    }
    for filter in filters
        .iter()
        .chain(event.tags.values("ignore").collect::<Vec<_>>().iter())
    {
        serde_json::from_str::<coracle_lib::filters::Filter>(filter).map_err(|error| {
            Refusal::invalid(format!("filter tag contains invalid JSON: {error}"))
        })?;
    }

    let callbacks: Vec<&str> = event.tags.values("callback").collect();
    let callback = match callbacks.as_slice() {
        [] => return Err(Refusal::invalid("missing callback tag")),
        [callback] if !callback.is_empty() => *callback,
        [_] => return Err(Refusal::invalid("empty callback tag")),
        _ => return Err(Refusal::invalid("too many callback tags")),
    };

    let url: url::Url = callback
        .parse()
        .map_err(|_| Refusal::invalid("callback must be a valid HTTP or HTTPS URL"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Refusal::invalid(
            "callback must be a valid HTTP or HTTPS URL",
        ));
    }

    if held >= MAX_PUSH_SUBSCRIPTIONS {
        return Err(Refusal::invalid("too many subscriptions registered"));
    }

    Ok(())
}
