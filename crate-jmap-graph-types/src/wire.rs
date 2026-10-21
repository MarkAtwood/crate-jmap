//! Private serde helpers shared by the Graph wire types.

use serde::de::Error as _;
use serde::Deserializer;
use serde_json::{Map, Value};

/// Insert `"@type": tag` into a serialized object.
pub(crate) fn with_tag(value: Value, tag: &str) -> Result<Value, serde_json::Error> {
    match value {
        Value::Object(mut map) => {
            map.insert("@type".to_owned(), Value::String(tag.to_owned()));
            Ok(Value::Object(map))
        }
        other => Err(serde::ser::Error::custom(format!(
            "@type-tagged value must serialize to a JSON object, got {other}"
        ))),
    }
}

/// Remove `@type` from `map` and return its string value.
pub(crate) fn take_tag<E: serde::de::Error>(map: &mut Map<String, Value>) -> Result<String, E> {
    match map.remove("@type") {
        Some(Value::String(s)) => Ok(s),
        Some(other) => Err(E::custom(format!("@type must be a string, found {other}"))),
        None => Err(E::missing_field("@type")),
    }
}

/// Deserialize a tagged payload from the map left after [`take_tag`].
pub(crate) fn from_map<'de, T, D>(map: Map<String, Value>) -> Result<T, D::Error>
where
    T: serde::de::DeserializeOwned,
    D: Deserializer<'de>,
{
    serde_json::from_value(Value::Object(map)).map_err(D::Error::custom)
}
