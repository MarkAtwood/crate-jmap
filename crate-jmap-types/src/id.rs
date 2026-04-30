//! RFC 8620 §1.2/§1.4 opaque string newtypes: [`Id`], [`UTCDate`], [`Date`], [`State`].

use serde::{Deserialize, Serialize};
use std::fmt;

/// Opaque non-empty server-assigned identifier (RFC 8620 §1.2).
///
/// Character set: URL-safe base64 alphabet (A-Za-z0-9, `-`, `_`), max 255 octets.
/// Clients MUST treat Id values as opaque strings — no parsing of structure.
// #[non_exhaustive] prevents callers from pattern-matching the inner field
// (e.g. `let Id(s) = id;`), preserving semver freedom to add fields later.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
#[non_exhaustive]
pub struct Id(String);

/// RFC 3339 UTC timestamp string (RFC 8620 §1.4).
///
/// Format: `YYYY-MM-DDTHH:MM:SSZ` — time-offset MUST be `Z`, letters uppercase,
/// fractional seconds omitted if zero. Example: `"2014-10-30T06:12:00Z"`.
// #[non_exhaustive] prevents callers from pattern-matching the inner field,
// preserving semver freedom to add fields later.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
#[non_exhaustive]
pub struct UTCDate(String);

/// RFC 3339 date-time string with any timezone offset (RFC 8620 §1.4).
///
/// Format: `YYYY-MM-DDTHH:MM:SS±HH:MM` or `Z` suffix — any valid RFC 3339 offset,
/// letters uppercase, fractional seconds omitted if zero.
/// Example: `"2014-10-30T14:12:00+08:00"`.
///
/// Distinct from [`UTCDate`], which requires the time-offset to be `Z`.
/// Use `Date` for fields derived from RFC 5322 email headers (e.g. `sentAt`),
/// which commonly carry non-UTC offsets.
// #[non_exhaustive] prevents callers from pattern-matching the inner field,
// preserving semver freedom to add fields later.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
#[non_exhaustive]
pub struct Date(String);

/// Opaque server state token (RFC 8620 §1.2).
///
/// Returned by `/get` and `/changes` methods. Clients echo it back in
/// `sinceState` / `ifInState` parameters. Treat as opaque — no structure assumed.
// #[non_exhaustive] prevents callers from pattern-matching the inner field,
// preserving semver freedom to add fields later.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
#[non_exhaustive]
pub struct State(String);

/// Generates `Display`, `From<String>`, `From<&str>`, `AsRef<str>`,
/// `PartialEq<str>`, `PartialEq<&str>`, and `into_inner` for a transparent
/// `String` newtype.
macro_rules! impl_string_newtype {
    ($T:ident) => {
        impl fmt::Display for $T {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
        impl From<String> for $T {
            fn from(s: String) -> Self {
                Self(s)
            }
        }
        impl From<&str> for $T {
            fn from(s: &str) -> Self {
                Self(s.to_owned())
            }
        }
        impl AsRef<str> for $T {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
        impl PartialEq<str> for $T {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }
        impl PartialEq<&str> for $T {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }
        impl $T {
            /// Consumes the value and returns the inner `String`.
            pub fn into_inner(self) -> String {
                self.0
            }
        }
    };
}

impl_string_newtype!(Id);
impl_string_newtype!(UTCDate);
impl_string_newtype!(Date);
impl_string_newtype!(State);

#[cfg(test)]
mod tests {
    use super::*;

    // Oracle: RFC 8620 §1.2 — Id is a plain JSON string, not a wrapped object.
    #[test]
    fn id_serializes_as_plain_string() {
        let id = Id("abc123".to_owned());
        let json = serde_json::to_string(&id).expect("serialize Id");
        assert_eq!(json, "\"abc123\"");
    }

    // Oracle: RFC 8620 §1.2 — Id round-trips through JSON.
    #[test]
    fn id_deserializes_from_plain_string() {
        let id: Id = serde_json::from_str("\"abc123\"").expect("deserialize Id");
        assert_eq!(id.as_ref(), "abc123");
    }

    // Oracle: RFC 8620 §1.4 example — "2014-10-30T06:12:00Z".
    #[test]
    fn utcdate_serializes_as_plain_string() {
        let d = UTCDate("2014-10-30T06:12:00Z".to_owned());
        let json = serde_json::to_string(&d).expect("serialize UTCDate");
        assert_eq!(json, "\"2014-10-30T06:12:00Z\"");
    }

    // Oracle: RFC 8620 §3.4.1 fixture — sessionState value is "75128aab4b1b".
    #[test]
    fn state_serializes_as_plain_string() {
        let s = State("75128aab4b1b".to_owned());
        let json = serde_json::to_string(&s).expect("serialize State");
        assert_eq!(json, "\"75128aab4b1b\"");
    }

    // Oracle: From<&str> trait contract.
    #[test]
    fn id_from_str() {
        let id = Id::from("hello");
        assert_eq!(id.as_ref(), "hello");
    }

    // Oracle: Display delegates to inner String.
    #[test]
    fn id_display() {
        let id = Id("display-test".to_owned());
        assert_eq!(id.to_string(), "display-test");
    }

    // Oracle: AsRef<str> returns the inner string.
    #[test]
    fn id_as_ref_str() {
        let id = Id("ref-test".to_owned());
        assert_eq!(id.as_ref(), "ref-test");
    }

    // Oracle: RFC 8620 §3.4.1 — State in sessionState field round-trips correctly.
    #[test]
    fn state_round_trip() {
        let s = State("75128aab4b1b".to_owned());
        let json = serde_json::to_string(&s).expect("serialize");
        let s2: State = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(s, s2);
    }

    // Oracle: RFC 8620 §1.4 example — Date allows non-UTC offsets, unlike UTCDate.
    #[test]
    fn date_accepts_non_utc_offset() {
        let d = Date("2014-10-30T14:12:00+08:00".to_owned());
        let json = serde_json::to_string(&d).expect("serialize Date");
        assert_eq!(json, "\"2014-10-30T14:12:00+08:00\"");
        let d2: Date = serde_json::from_str(&json).expect("deserialize Date");
        assert_eq!(d, d2);
    }
}
