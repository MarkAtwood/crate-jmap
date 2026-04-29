use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Deserialize a JSON field into `Some(Clearable<T>)`, where:
/// - JSON `null`  → `Some(Clearable::Clear)`
/// - JSON value   → `Some(Clearable::Set(v))`
///
/// Pair with `#[serde(default)]` so that an absent field yields `None`.
///
/// # Note
/// Referenced via string path in `#[serde(deserialize_with = "some_clearable")]`.
/// rustc cannot track such string references, so the compiler incorrectly reports
/// this function as dead code without the allow attribute.
#[allow(dead_code)]
pub(crate) fn some_clearable<'de, D, T>(d: D) -> Result<Option<Clearable<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    Ok(Some(Clearable::deserialize(d)?))
}

/// Represents a JSON field that distinguishes `null` (explicit clear) from absence (unchanged).
///
/// Use as `Option<Clearable<T>>` where:
/// - `None` = field absent (no change)
/// - `Some(Clearable::Clear)` = field present as `null` (clear the value)
/// - `Some(Clearable::Set(v))` = field present with value `v`
///
/// Not `#[non_exhaustive]` — callers must be able to match exhaustively on `Clear` / `Set`.
#[derive(Debug, Clone, PartialEq)]
pub enum Clearable<T> {
    /// Field was present in JSON as `null` — explicitly clear the value.
    Clear,
    /// Field was present in JSON with a value.
    Set(T),
}

impl<T: Serialize> Serialize for Clearable<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Clearable::Set(v) => v.serialize(s),
            Clearable::Clear => s.serialize_none(),
        }
    }
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for Clearable<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match Option::<T>::deserialize(d)? {
            None => Clearable::Clear,
            Some(v) => Clearable::Set(v),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clearable_null_to_clear() {
        let c: Clearable<String> = serde_json::from_str("null").unwrap();
        assert_eq!(c, Clearable::Clear);
    }

    #[test]
    fn clearable_value_to_set() {
        let c: Clearable<String> = serde_json::from_str("\"hello\"").unwrap();
        assert_eq!(c, Clearable::Set("hello".to_owned()));
    }

    #[test]
    fn clearable_set_serializes_to_value() {
        let c = Clearable::Set("hello".to_owned());
        assert_eq!(serde_json::to_string(&c).unwrap(), "\"hello\"");
    }

    #[test]
    fn clearable_clear_serializes_to_null() {
        let c: Clearable<String> = Clearable::Clear;
        assert_eq!(serde_json::to_string(&c).unwrap(), "null");
    }

    #[test]
    fn option_clearable_none_absent() {
        // When used as Option<Clearable<T>>, None means field is absent from JSON.
        // This test verifies the field is not emitted when None.
        use serde::{Deserialize, Serialize};
        #[derive(Serialize, Deserialize)]
        struct Wrapper {
            #[serde(skip_serializing_if = "Option::is_none")]
            value: Option<Clearable<String>>,
        }
        let w = Wrapper { value: None };
        let json = serde_json::to_string(&w).unwrap();
        assert!(!json.contains("value"), "field must be absent when None");
    }
}
