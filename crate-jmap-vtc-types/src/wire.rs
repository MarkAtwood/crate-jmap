//! Private serde helpers shared by the VTC wire types.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};

/// Deserialize a property that may be absent, `null` or a value into
/// `Option<Option<T>>`: absent is `None` (via `#[serde(default)]`), `null`
/// is `Some(None)` and a value is `Some(Some(v))`. Paired with
/// `skip_serializing_if = "Option::is_none"`, the three states round-trip.
pub(crate) fn present<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

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
        Some(other) => Err(E::custom(format!(
            "@type must be a string, found {}",
            jmap_types::json_type_name(&other)
        ))),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle: hand-written JSON whose `@type` carries a canary. RFC 8620
    /// §3.6.1 makes an error `description` "unstructured English text
    /// suitable for displaying to a developer", so the client value must not
    /// be echoed; only its JSON type (RFC 8259 §3 "object") is named.
    #[test]
    fn take_tag_error_does_not_echo_value() {
        let mut map = serde_json::json!({ "@type": { "note": "CANARY-kwqn" } })
            .as_object()
            .cloned()
            .unwrap_or_default();
        let err = take_tag::<serde_json::Error>(&mut map)
            .unwrap_err()
            .to_string();
        assert!(!err.contains("CANARY-kwqn"), "{err}");
        assert_eq!(err, "@type must be a string, found object");
    }
}
