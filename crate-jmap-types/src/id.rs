//! RFC 8620 §1.2/§1.4 opaque string newtypes: [`Id`], [`UTCDate`], [`Date`], [`State`].

use serde::{Deserialize, Serialize};
use std::fmt;

/// Error returned by the fallible constructors [`Id::new_validated`],
/// [`UTCDate::new_validated`], and [`State::new_validated`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
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
///
/// # Deserialization is NOT validated
///
/// `UTCDate` is `#[serde(transparent)]` over `String`. Any string that
/// deserializes into a `String` deserializes into a `UTCDate` — including
/// `"not-a-date"`, `"2024-01-19T18:00:00"` (no `Z` suffix), or any other
/// shape that violates RFC 8620 §1.4. The newtype carries the
/// **type-level intent** of "RFC 8620 UTC timestamp" but does NOT enforce
/// it at the wire boundary.
///
/// Use [`UTCDate::new_validated`] when constructing from untrusted input,
/// or call [`UTCDate::to_epoch_seconds`] when consuming a deserialized
/// value: the latter re-validates structural format AND semantic ranges
/// (month, day, hour, minute, second) and returns [`ValidationError`] on
/// any deviation. Treat any field of type `Option<UTCDate>` arriving
/// from a peer as "RFC 8620 timestamp by convention, not by contract".
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
///
/// # Deserialization is NOT validated
///
/// `Date` is `#[serde(transparent)]` over `String`. Any string that
/// deserializes into a `String` deserializes into a `Date`, including
/// values that violate RFC 8620 §1.4 / RFC 3339. The newtype carries
/// the **type-level intent** but does NOT enforce it at the wire
/// boundary. Treat any field of type `Option<Date>` arriving from a
/// peer as "RFC 8620 timestamp by convention, not by contract".
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
///
/// # Migrating from `String`-typed code
///
/// Earlier revisions of this crate exposed state fields as
/// `pub session_state: String` on [`crate::JmapResponse`] and similar
/// shapes. The current revision uses the `State` newtype for type
/// safety. The wire format is unchanged (`State` is
/// `#[serde(transparent)]` over `String`). The Rust API conversions
/// available to callers migrating from `String`:
///
/// - Read as `&str`: `state.as_ref()` (via `AsRef<str>`) or
///   `format!("{state}")` (via `Display`).
/// - Compare to a literal: `state == "abc"` (via `PartialEq<str>`).
/// - Move into an owned `String`: `state.into_inner()`.
/// - Borrow as `&String`: not directly available; use `as_ref()` to
///   get `&str` instead.
/// - Build from `&str` or `String`: `State::from("abc")` or
///   `State::from(my_string)` (via `From<&str>` / `From<String>`).
///
/// Callers passing a `State` to a function that takes `String` should
/// either change the signature to `impl AsRef<str>` or call
/// `state.into_inner()` at the boundary. Callers keying a
/// `HashMap<String, _>` by state token can either rekey by
/// `HashMap<State, _>` (`State` implements `Hash + Eq`) or call
/// `state.into_inner()` at insertion / lookup time.
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
/// RFC 8620 §1.2 requires Ids to use the "URL and Filename Safe" base64
/// alphabet defined in RFC 4648 §5, excluding the pad character `=`.
/// That is: ASCII alphanumeric characters (`A-Z`, `a-z`, `0-9`), hyphen
/// (`-`), and underscore (`_`). The string must also be non-empty and
/// at most 255 bytes.
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
        // RFC 4648 §5 URL-safe base64 alphabet, minus the pad `=`:
        // A-Z, a-z, 0-9, '-', '_'.
        if !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_' {
            return Err(ValidationError(format!(
                "Id contains invalid character {:?} (U+{:04X})",
                ch, ch as u32
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
    /// RFC 8620 §1.2 restricts Ids to the URL-safe base64 alphabet defined
    /// in RFC 4648 §5, excluding the pad character `=`. The permitted
    /// characters are therefore the ASCII alphanumerics (`A-Z`, `a-z`,
    /// `0-9`), hyphen (`-`), and underscore (`_`). The string must also
    /// be non-empty and at most 255 bytes.
    ///
    /// # Errors
    ///
    /// Returns [`ValidationError`] when the input does not satisfy
    /// RFC 8620 §1.2. The error description contains a category keyword
    /// that callers can match on if they need finer-grained handling:
    ///
    /// - `"empty"` — the input was an empty string;
    /// - `"exceeds 255 bytes"` — the input was longer than the spec
    ///   maximum;
    /// - `"invalid character"` — the input contained at least one
    ///   character outside `A-Z`, `a-z`, `0-9`, `-`, `_`.
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

    /// Convert this [`UTCDate`] to seconds since the Unix epoch
    /// (`1970-01-01T00:00:00Z`).
    ///
    /// Re-validates the structural RFC 8620 §1.4 format (the value may have
    /// been constructed via [`UTCDate::from`] without validation) and also
    /// validates semantic ranges: month `1..=12`, day `1..=days_in_month`,
    /// hour `0..=23`, minute `0..=59`, second `0..=59` (no leap seconds).
    /// Returns [`ValidationError`] on any validation failure.
    ///
    /// Negative values are returned for dates before `1970-01-01T00:00:00Z`.
    ///
    /// Uses the proleptic Gregorian calendar via Hinnant's `days_from_civil`
    /// algorithm, which is exact-integer and handles leap years and century
    /// rules correctly. No external dependencies.
    ///
    /// # Examples
    ///
    /// ```
    /// use jmap_types::UTCDate;
    /// let d = UTCDate::new_validated("1970-01-01T00:00:00Z").unwrap();
    /// assert_eq!(d.to_epoch_seconds().unwrap(), 0);
    /// let rfc = UTCDate::new_validated("2014-10-30T06:12:00Z").unwrap();
    /// assert_eq!(rfc.to_epoch_seconds().unwrap(), 1_414_649_520);
    /// ```
    pub fn to_epoch_seconds(&self) -> Result<i64, ValidationError> {
        utcdate_to_epoch_seconds(&self.0)
    }
}

/// Number of days in `month` of `year`, accounting for proleptic Gregorian
/// leap-year rules (year divisible by 4, except centuries not divisible by 400).
fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 0, // unreachable when month range is validated by caller
    }
}

/// Hinnant's `days_from_civil` algorithm: number of days from
/// `1970-01-01` (positive) or to `1970-01-01` (negative) for the proleptic
/// Gregorian date `(y, m, d)`. Source: Howard Hinnant, "chrono-Compatible
/// Low-Level Date Algorithms", §6.
///
/// Preconditions (caller-validated): `m` in `1..=12`, `d` in
/// `1..=days_in_month(y, m)`. Years are unbounded `i64`.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let m = m as i64;
    let d = d as i64;
    let mp = if m > 2 { m - 3 } else { m + 9 }; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719_468
}

/// Internal helper: convert a `YYYY-MM-DDTHH:MM:SSZ` string to epoch seconds.
fn utcdate_to_epoch_seconds(s: &str) -> Result<i64, ValidationError> {
    validate_utcdate(s)?;
    // validate_utcdate guarantees ASCII-digit positions; direct parse is safe.
    let parse = |start: usize, end: usize| -> i64 {
        let mut n: i64 = 0;
        for &b in &s.as_bytes()[start..end] {
            n = n * 10 + (b - b'0') as i64;
        }
        n
    };
    let year = parse(0, 4);
    let month = parse(5, 7) as u32;
    let day = parse(8, 10) as u32;
    let hour = parse(11, 13) as u32;
    let minute = parse(14, 16) as u32;
    let second = parse(17, 19) as u32;

    if !(1..=12).contains(&month) {
        return Err(ValidationError(format!(
            "UTCDate month must be 1..=12, got {month} in {s:?}"
        )));
    }
    let max_day = days_in_month(year, month);
    if !(1..=max_day).contains(&day) {
        return Err(ValidationError(format!(
            "UTCDate day must be 1..={max_day} for {year:04}-{month:02}, got {day} in {s:?}"
        )));
    }
    if hour > 23 {
        return Err(ValidationError(format!(
            "UTCDate hour must be 0..=23, got {hour} in {s:?}"
        )));
    }
    if minute > 59 {
        return Err(ValidationError(format!(
            "UTCDate minute must be 0..=59, got {minute} in {s:?}"
        )));
    }
    // No leap seconds in RFC 8620 §1.4 (it specifies seconds 0..=59).
    if second > 59 {
        return Err(ValidationError(format!(
            "UTCDate second must be 0..=59, got {second} in {s:?}"
        )));
    }

    let days = days_from_civil(year, month, day);
    Ok(days * 86_400 + hour as i64 * 3_600 + minute as i64 * 60 + second as i64)
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

    /// Oracle: RFC 8620 §1.2 URL-safe base64 alphabet — space (0x20) is
    /// not in `A-Za-z0-9-_`.
    #[test]
    fn id_new_validated_space_fails() {
        let err = Id::new_validated("has space").unwrap_err();
        assert!(err.0.contains("invalid character"), "{err}");
    }

    /// Oracle: RFC 8620 §1.2 URL-safe base64 alphabet — double-quote
    /// (0x22) is not in `A-Za-z0-9-_`.
    #[test]
    fn id_new_validated_dquote_fails() {
        let err = Id::new_validated("has\"quote").unwrap_err();
        assert!(err.0.contains("invalid character"), "{err}");
    }

    /// Oracle: RFC 8620 §1.2 URL-safe base64 alphabet — a control
    /// character (0x01) is not in `A-Za-z0-9-_`.
    #[test]
    fn id_new_validated_control_char_fails() {
        let err = Id::new_validated("has\x01ctrl").unwrap_err();
        assert!(err.0.contains("invalid character"), "{err}");
    }

    /// Oracle: RFC 8620 §1.2 URL-safe base64 alphabet (RFC 4648 §5) —
    /// every visible-ASCII character outside `A-Za-z0-9-_` MUST be
    /// rejected. These characters were accepted by the previous
    /// (incorrect) `SAFE-CHAR` validator; this test pins the spec-
    /// conforming rejection (bd:JMAP-6xs8.19).
    #[test]
    fn id_new_validated_rejects_non_url_safe_base64_chars() {
        // Representative sample of characters that are visible ASCII
        // (the old SAFE-CHAR set) but NOT in the RFC 4648 §5 URL-safe
        // base64 alphabet. Each MUST be rejected with category
        // "invalid character".
        let rejected = [
            '!', '#', '$', '%', '&', '\'', '(', ')', '*', '+', ',', '.', '/',
            ':', ';', '<', '=', '>', '?', '@', '[', '\\', ']', '^', '`', '{',
            '|', '}', '~',
        ];
        for ch in rejected {
            let input = format!("a{ch}z");
            let err = Id::new_validated(&input).unwrap_err();
            assert!(
                err.0.contains("invalid character"),
                "char {ch:?} (input {input:?}) must be rejected with \
                 'invalid character', got: {err}"
            );
        }
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

    // -----------------------------------------------------------------------
    // UTCDate::to_epoch_seconds tests
    //
    // Oracle for all numeric vectors: Python 3 `datetime.datetime(...).
    // replace(tzinfo=timezone.utc).timestamp()`, computed independently and
    // hardcoded as integer literals per workspace AGENTS.md "Test vector
    // discipline". The oracle is independent of the code under test — these
    // are NOT round-trip self-tests.
    // -----------------------------------------------------------------------

    /// Oracle (Python): `1970-01-01T00:00:00Z` → 0.
    #[test]
    fn utcdate_to_epoch_seconds_unix_epoch() {
        let d = UTCDate::new_validated("1970-01-01T00:00:00Z").unwrap();
        assert_eq!(d.to_epoch_seconds().unwrap(), 0);
    }

    /// Oracle (Python): RFC 8620 §1.4 example `2014-10-30T06:12:00Z` →
    /// 1_414_649_520.
    #[test]
    fn utcdate_to_epoch_seconds_rfc8620_example() {
        let d = UTCDate::new_validated("2014-10-30T06:12:00Z").unwrap();
        assert_eq!(d.to_epoch_seconds().unwrap(), 1_414_649_520);
    }

    /// Oracle (Python): `2000-01-01T00:00:00Z` (Y2K, century-leap year) →
    /// 946_684_800.
    #[test]
    fn utcdate_to_epoch_seconds_y2k() {
        let d = UTCDate::new_validated("2000-01-01T00:00:00Z").unwrap();
        assert_eq!(d.to_epoch_seconds().unwrap(), 946_684_800);
    }

    /// Oracle (Python): `1999-12-31T23:59:59Z` (one second before Y2K) →
    /// 946_684_799.
    #[test]
    fn utcdate_to_epoch_seconds_pre_y2k() {
        let d = UTCDate::new_validated("1999-12-31T23:59:59Z").unwrap();
        assert_eq!(d.to_epoch_seconds().unwrap(), 946_684_799);
    }

    /// Oracle (Python): `2024-02-29T00:00:00Z` (leap year, leap day start) →
    /// 1_709_164_800.
    #[test]
    fn utcdate_to_epoch_seconds_leap_day_2024() {
        let d = UTCDate::new_validated("2024-02-29T00:00:00Z").unwrap();
        assert_eq!(d.to_epoch_seconds().unwrap(), 1_709_164_800);
    }

    /// Oracle (Python): `2024-02-29T23:59:59Z` (leap day end) →
    /// 1_709_251_199.
    #[test]
    fn utcdate_to_epoch_seconds_leap_day_2024_end() {
        let d = UTCDate::new_validated("2024-02-29T23:59:59Z").unwrap();
        assert_eq!(d.to_epoch_seconds().unwrap(), 1_709_251_199);
    }

    /// Oracle (Python): `2100-03-01T00:00:00Z` exercises the
    /// century-non-leap-year rule (2100 is not a leap year, divisible by 100
    /// but not 400).
    #[test]
    fn utcdate_to_epoch_seconds_2100_non_leap_century() {
        let d = UTCDate::new_validated("2100-03-01T00:00:00Z").unwrap();
        assert_eq!(d.to_epoch_seconds().unwrap(), 4_107_542_400);
    }

    /// Oracle (Python): `1969-12-31T23:59:59Z` (one second before epoch) → -1.
    /// Verifies negative-epoch handling.
    #[test]
    fn utcdate_to_epoch_seconds_one_before_epoch() {
        let d = UTCDate::new_validated("1969-12-31T23:59:59Z").unwrap();
        assert_eq!(d.to_epoch_seconds().unwrap(), -1);
    }

    /// Oracle (Python): `1900-01-01T00:00:00Z` → -2_208_988_800.
    /// 1900 is divisible by 100 but not 400, so NOT a leap year — exercises
    /// the same century rule as 2100 but well before the epoch.
    #[test]
    fn utcdate_to_epoch_seconds_1900() {
        let d = UTCDate::new_validated("1900-01-01T00:00:00Z").unwrap();
        assert_eq!(d.to_epoch_seconds().unwrap(), -2_208_988_800);
    }

    /// Oracle (Python): `0001-01-01T00:00:00Z` → -62_135_596_800.
    /// Far-past boundary; verifies the proleptic Gregorian algorithm
    /// extrapolates correctly to year 1.
    #[test]
    fn utcdate_to_epoch_seconds_year_one() {
        let d = UTCDate::new_validated("0001-01-01T00:00:00Z").unwrap();
        assert_eq!(d.to_epoch_seconds().unwrap(), -62_135_596_800);
    }

    /// Oracle (Python): `9999-12-31T23:59:59Z` (UTCDate max year) →
    /// 253_402_300_799.
    /// Far-future boundary within `i64` range.
    #[test]
    fn utcdate_to_epoch_seconds_year_9999() {
        let d = UTCDate::new_validated("9999-12-31T23:59:59Z").unwrap();
        assert_eq!(d.to_epoch_seconds().unwrap(), 253_402_300_799);
    }

    /// Oracle (Python): `2038-01-19T03:14:07Z` = i32::MAX = 2_147_483_647
    /// (the "Year 2038 problem" boundary).
    #[test]
    fn utcdate_to_epoch_seconds_y2038_boundary() {
        let d = UTCDate::new_validated("2038-01-19T03:14:07Z").unwrap();
        assert_eq!(d.to_epoch_seconds().unwrap(), 2_147_483_647);
    }

    /// Oracle (Python): `2038-01-19T03:14:08Z` = i32::MAX + 1.
    /// Verifies `i64` handles the i32 overflow point that 32-bit time_t
    /// implementations would wrap at.
    #[test]
    fn utcdate_to_epoch_seconds_post_y2038() {
        let d = UTCDate::new_validated("2038-01-19T03:14:08Z").unwrap();
        assert_eq!(d.to_epoch_seconds().unwrap(), 2_147_483_648);
    }

    /// Oracle: `validate_utcdate` accepts month 13 structurally; semantic
    /// validation in `to_epoch_seconds` must reject it.
    #[test]
    fn utcdate_to_epoch_seconds_rejects_month_13() {
        let d = UTCDate::from("2024-13-01T00:00:00Z");
        let err = d.to_epoch_seconds().unwrap_err();
        assert!(err.0.contains("month"), "error must mention month: {err}");
    }

    /// Oracle: `validate_utcdate` accepts day 32 structurally; semantic
    /// validation in `to_epoch_seconds` must reject it.
    #[test]
    fn utcdate_to_epoch_seconds_rejects_day_32() {
        let d = UTCDate::from("2024-01-32T00:00:00Z");
        let err = d.to_epoch_seconds().unwrap_err();
        assert!(err.0.contains("day"), "error must mention day: {err}");
    }

    /// Oracle: 2023 is NOT a leap year; Feb 29 must be rejected as
    /// out-of-range for that month.
    #[test]
    fn utcdate_to_epoch_seconds_rejects_feb_29_non_leap() {
        let d = UTCDate::from("2023-02-29T00:00:00Z");
        let err = d.to_epoch_seconds().unwrap_err();
        assert!(err.0.contains("day"), "error must mention day: {err}");
    }

    /// Oracle: hour 24 is out of range per RFC 8620 §1.4 (no end-of-day
    /// convention) — `to_epoch_seconds` must reject it.
    #[test]
    fn utcdate_to_epoch_seconds_rejects_hour_24() {
        let d = UTCDate::from("2024-01-01T24:00:00Z");
        let err = d.to_epoch_seconds().unwrap_err();
        assert!(err.0.contains("hour"), "error must mention hour: {err}");
    }

    /// Oracle: minute 60 out of range.
    #[test]
    fn utcdate_to_epoch_seconds_rejects_minute_60() {
        let d = UTCDate::from("2024-01-01T00:60:00Z");
        let err = d.to_epoch_seconds().unwrap_err();
        assert!(err.0.contains("minute"), "error must mention minute: {err}");
    }

    /// Oracle: second 60 (leap second) is not permitted by RFC 8620 §1.4 —
    /// `to_epoch_seconds` must reject it.
    #[test]
    fn utcdate_to_epoch_seconds_rejects_leap_second() {
        let d = UTCDate::from("2016-12-31T23:59:60Z");
        let err = d.to_epoch_seconds().unwrap_err();
        assert!(err.0.contains("second"), "error must mention second: {err}");
    }

    /// Oracle: a value constructed via `UTCDate::from` with wrong structure
    /// must surface a structural ValidationError (re-validation in
    /// `to_epoch_seconds`).
    #[test]
    fn utcdate_to_epoch_seconds_rejects_invalid_structure() {
        let d = UTCDate::from("not-a-date");
        assert!(d.to_epoch_seconds().is_err());
    }

    /// Oracle (Python): `2024-02-28T23:59:59Z` → `2024-02-29T00:00:00Z` is
    /// exactly one second forward. Verifies leap-day arithmetic is
    /// internally consistent.
    #[test]
    fn utcdate_to_epoch_seconds_leap_day_boundary_one_sec() {
        let a = UTCDate::new_validated("2024-02-28T23:59:59Z").unwrap();
        let b = UTCDate::new_validated("2024-02-29T00:00:00Z").unwrap();
        let diff = b.to_epoch_seconds().unwrap() - a.to_epoch_seconds().unwrap();
        assert_eq!(
            diff, 1,
            "Feb 28 23:59:59 → Feb 29 00:00:00 must be 1 second"
        );
    }

    /// Oracle (Python): a 365-day duration `2023-01-01T00:00:00Z` to
    /// `2024-01-01T00:00:00Z` is exactly `365 * 86_400 = 31_536_000` seconds
    /// (2023 is not a leap year). Anchors the `CalendarsLimits::default()
    /// .max_expanded_query_duration_seconds` value (`31_536_000`).
    #[test]
    fn utcdate_to_epoch_seconds_365_day_year_duration() {
        let a = UTCDate::new_validated("2023-01-01T00:00:00Z").unwrap();
        let b = UTCDate::new_validated("2024-01-01T00:00:00Z").unwrap();
        let diff = b.to_epoch_seconds().unwrap() - a.to_epoch_seconds().unwrap();
        assert_eq!(diff, 31_536_000);
    }

    /// Oracle (Python): a 366-day duration `2024-01-01T00:00:00Z` to
    /// `2025-01-01T00:00:00Z` is `366 * 86_400 = 31_622_400` seconds (2024 is
    /// a leap year).
    #[test]
    fn utcdate_to_epoch_seconds_366_day_leap_year_duration() {
        let a = UTCDate::new_validated("2024-01-01T00:00:00Z").unwrap();
        let b = UTCDate::new_validated("2025-01-01T00:00:00Z").unwrap();
        let diff = b.to_epoch_seconds().unwrap() - a.to_epoch_seconds().unwrap();
        assert_eq!(diff, 31_622_400);
    }
}
