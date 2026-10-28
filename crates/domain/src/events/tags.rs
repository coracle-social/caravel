//! Tags and ids as the event store reads and writes them.

pub fn tag(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

pub fn decode_id(hex: &str) -> Option<[u8; 32]> {
    hex::decode(hex).ok()?.try_into().ok()
}
