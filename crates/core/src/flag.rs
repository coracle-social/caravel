//! Serde for boolean settings, which the API carries as `0`/`1` and accepts as
//! either numbers or booleans.

use serde::{Deserialize, Deserializer, Serializer};

pub fn serialize<S: Serializer>(value: &bool, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_u8(u8::from(*value))
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    Ok(Flag::deserialize(deserializer)?.0)
}

/// A flag that deserializes from `0`/`1` or `false`/`true`, for request bodies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flag(pub bool);

impl<'de> Deserialize<'de> for Flag {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Bool(bool),
            Int(i64),
        }

        Ok(Flag(match Raw::deserialize(deserializer)? {
            Raw::Bool(value) => value,
            Raw::Int(value) => value == 1,
        }))
    }
}
