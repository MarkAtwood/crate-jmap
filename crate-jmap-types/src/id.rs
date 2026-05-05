//! RFC 8620 §1.2/§1.4 opaque string newtypes: [`Id`], [`UTCDate`], [`Date`], [`State`].

use serde::{Deserialize, Serialize};
use std::fmt;

/// Error returned by the fallible constructors [`Id::new_validated`],
/// [`UTCDate::new_validated`], and [`State::new_validated`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError(pub String);

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ValidationError {}

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
        impl std::borrow::Borrow<str> for $T {
            fn borrow(&self) -> &str {
                &self.0
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

// ---------------------------------------------------------------------------
// Fallible constructors — validate RFC 8620 constraints at the boundary.
//
// These are named constructors (not TryFrom impls) because Id/UTCDate/State
// already implement From<String> and From<&str>.  Rust's blanket impl
// `impl<T,U> TryFrom<U> where U: Into<T>` would make TryFrom<String>
// infallible (Error = Infallible) via the existing From impl, making it
// impossible to add a second, fallible TryFrom<String>.  Named constructors
// achieve the same goal without the conflict.
// ---------------------------------------------------------------------------

/// Validate an [`Id`] string per RFC 8620 §1.2.
///
/// SAFE-CHAR = %x21 / %x23-7E (visible ASCII, no SPACE, no DEL, no DQUOTE).
/// Must be non-empty and at most 255 bytes.
fn validate_id(s: &str) -> Result<(), ValidationError> {
    if s.is_empty() {
        return Err(ValidationError("Id must not be empty".into()));
    }
    if s.len() > 255 {
        return Err(ValidationError(format!(
            "Id exceeds 255 bytes (got {})",
            s.len()
        )));
    }
    for ch in s.chars() {
        let b = ch as u32;
        // SAFE-CHAR: 0x21 through 0x7E, excluding 0x22 (DQUOTE).
        if !(0x21..=0x7E).contains(&b) || b == 0x22 {
            return Err(ValidationError(format!(
                "Id contains invalid character {:?} (U+{b:04X})",
                ch
            )));
        }
    }
    Ok(())
}

/// Validate a [`UTCDate`] string per RFC 8620 §1.4.
///
/// Required format: `YYYY-MM-DDTHH:MM:SSZ` (exactly 20 characters, `Z` suffix,
/// all digit positions ASCII digits). No external crate needed.
fn validate_utcdate(s: &str) -> Result<(), ValidationError> {
    if s.len() != 20 {
        return Err(ValidationError(format!(
            "UTCDate must be exactly 20 characters (YYYY-MM-DDTHH:MM:SSZ), got {:?}",
            s
        )));
    }
    let b = s.as_bytes();
    // Fixed separators: dashes, T, colons, Z.
    if b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b[19] != b'Z'
    {
        return Err(ValidationError(format!(
            "UTCDate has wrong structure, expected YYYY-MM-DDTHH:MM:SSZ, got {:?}",
            s
        )));
    }
    // Digit positions: 0-3 (year), 5-6 (month), 8-9 (day),
    //                  11-12 (hour), 14-15 (min), 17-18 (sec).
    for &pos in &[0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18] {
        if !b[pos].is_ascii_digit() {
            return Err(ValidationError(format!(
                "UTCDate position {} is not a digit in {:?}",
                pos, s
            )));
        }
    }
    Ok(())
}

/// Validate a [`State`] string: must be non-empty.
///
/// RFC 8620 §1.2 does not restrict the character set for State beyond
/// requiring it to be non-empty.
fn validate_state(s: &str) -> Result<(), ValidationError> {
    if s.is_empty() {
        return Err(ValidationError("State must not be empty".into()));
    }
    Ok(())
}

impl Id {
    /// Construct an [`Id`] with RFC 8620 §1.2 syntax validation.
    ///
    /// Rejects empty strings, strings longer than 255 bytes, and strings
    /// containing characters outside the SAFE-CHAR set (`%x21 / %x23-7E` —
    /// visible ASCII excluding `"`).
    ///
    /// Use [`Id::from`] when the value is known to be valid (e.g. a string
    /// received from a JMAP server response).
    pub fn new_validated(s: impl Into<String>) -> Result<Self, ValidationError> {
        let s = s.into();
        validate_id(&s)?;
        Ok(Self(s))
    }
}

impl UTCDate {
    /// Construct a [`UTCDate`] with RFC 8620 §1.4 format validation.
    ///
    /// Requires exactly the format `YYYY-MM-DDTHH:MM:SSZ` (20 characters,
    /// `Z` suffix, all numeric fields are ASCII digits).
    ///
    /// Use [`UTCDate::from`] when the value is known to be valid.
    pub fn new_validated(s: impl Into<String>) -> Result<Self, ValidationError> {
        let s = s.into();
        validate_utcdate(&s)?;
        Ok(Self(s))
    }
}

impl State {
    /// Construct a [`State`] with RFC 8620 §1.2 validation.
    ///
    /// Rejects empty strings. RFC 8620 §1.2 requires State to be non-empty;
    /// no character-set restriction is imposed.
    ///
    /// Use [`State::from`] when the value is known to be valid.
    pub fn new_validated(s: impl Into<String>) -> Result<Self, ValidationError> {
        let s = s.into();
        validate_state(&s)?;
        Ok(Self(s))
    }
}

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

    // -----------------------------------------------------------------------
    // new_validated / ValidationError tests
    // Oracle for all: RFC 8620 §1.2 (Id, State) and §1.4 (UTCDate).
    // -----------------------------------------------------------------------

    /// Oracle: RFC 8620 §1.2 — Id must not be empty.
    #[test]
    fn id_new_validated_empty_fails() {
        let err = Id::new_validated("").unwrap_err();
        assert!(err.0.contains("empty"), "error must mention 'empty': {err}");
    }

    /// Oracle: RFC 8620 §1.2 SAFE-CHAR — space (0x20) is not allowed.
    #[test]
    fn id_new_validated_space_fails() {
        let err = Id::new_validated("has space").unwrap_err();
        assert!(err.0.contains("invalid character"), "{err}");
    }

    /// Oracle: RFC 8620 §1.2 SAFE-CHAR — double-quote (0x22) is excluded.
    #[test]
    fn id_new_validated_dquote_fails() {
        let err = Id::new_validated("has\"quote").unwrap_err();
        assert!(err.0.contains("invalid character"), "{err}");
    }

    /// Oracle: control character (0x01) is not in SAFE-CHAR.
    #[test]
    fn id_new_validated_control_char_fails() {
        let err = Id::new_validated("has\x01ctrl").unwrap_err();
        assert!(err.0.contains("invalid character"), "{err}");
    }

    /// Oracle: RFC 8620 §1.2 — max length 255 bytes.
    #[test]
    fn id_new_validated_too_long_fails() {
        let long = "a".repeat(256);
        assert!(Id::new_validated(long).is_err());
    }

    /// Oracle: valid printable ASCII Id succeeds and is preserved verbatim.
    #[test]
    fn id_new_validated_valid_succeeds() {
        let id = Id::new_validated("abc123-_ABC").expect("valid Id must succeed");
        assert_eq!(id.as_ref(), "abc123-_ABC");
    }

    /// Oracle: exactly 255-byte Id succeeds.
    #[test]
    fn id_new_validated_max_length_succeeds() {
        let id255 = "a".repeat(255);
        Id::new_validated(id255).expect("255-byte Id must succeed");
    }

    /// Oracle: RFC 8620 §1.4 example "2014-10-30T06:12:00Z" must succeed.
    #[test]
    fn utcdate_new_validated_valid_succeeds() {
        let d = UTCDate::new_validated("2014-10-30T06:12:00Z").expect("valid UTCDate must succeed");
        assert_eq!(d.as_ref(), "2014-10-30T06:12:00Z");
    }

    /// Oracle: UTC date without Z suffix is not RFC 8620 §1.4 format.
    #[test]
    fn utcdate_new_validated_no_z_fails() {
        assert!(UTCDate::new_validated("2014-10-30T06:12:00+00:00").is_err());
    }

    /// Oracle: empty UTCDate fails.
    #[test]
    fn utcdate_new_validated_empty_fails() {
        assert!(UTCDate::new_validated("").is_err());
    }

    /// Oracle: UTCDate with wrong length fails.
    #[test]
    fn utcdate_new_validated_wrong_length_fails() {
        assert!(UTCDate::new_validated("2014-10-30").is_err());
        // fractional seconds are not permitted in RFC 8620 §1.4 format.
        assert!(UTCDate::new_validated("2014-10-30T06:12:00.000Z").is_err());
    }

    /// Oracle: non-digit in year position fails.
    #[test]
    fn utcdate_new_validated_non_digit_fails() {
        assert!(UTCDate::new_validated("XXXX-10-30T06:12:00Z").is_err());
    }

    /// Oracle: RFC 8620 §1.2 — State must be non-empty.
    #[test]
    fn state_new_validated_empty_fails() {
        let err = State::new_validated("").unwrap_err();
        assert!(err.0.contains("empty"), "{err}");
    }

    /// Oracle: non-empty State string succeeds.
    #[test]
    fn state_new_validated_valid_succeeds() {
        let s = State::new_validated("75128aab4b1b").expect("valid State must succeed");
        assert_eq!(s.as_ref(), "75128aab4b1b");
    }

    /// Oracle: ValidationError implements std::error::Error and Display.
    #[test]
    fn validation_error_implements_error() {
        let e = Id::new_validated("").unwrap_err();
        let _: &dyn std::error::Error = &e;
        assert!(!e.to_string().is_empty(), "error message must not be empty");
        assert_eq!(format!("{e}"), e.0, "Display must show the inner message");
    }
}
