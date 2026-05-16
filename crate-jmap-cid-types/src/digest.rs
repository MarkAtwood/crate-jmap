//! `Sha256` — the SHA-256 digest wire shape defined by
//! draft-atwood-jmap-cid-00 §2.
//!
//! Spec text (§2 Conventions):
//!
//! > `sha256-value = 64( %x30-39 / %x61-66 )`
//! > ; lowercase hex, exactly 64 chars
//!
//! Wire format is the bare 64-character lowercase hex string — NOT a
//! wrapped JSON object. Round-trips bit-for-bit when the input is
//! already a canonical (lowercase) hex string. Uppercase hex,
//! non-hex characters, and any length other than 64 are rejected at
//! deserialize and at [`Sha256::from_hex`].
//!
//! This crate intentionally carries no SHA-256 *computation* — only
//! the wire shape. Servers / consumers compute the digest themselves
//! (typically via [`sha2`](https://crates.io/crates/sha2) or
//! [`ring`](https://crates.io/crates/ring)) and pass the 32 raw
//! bytes to [`Sha256::from_raw_digest`] to format the wire value.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Reason a candidate string failed to parse as a [`Sha256`].
///
/// Returned inside [`Sha256DigestError`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Sha256DigestErrorKind {
    /// The candidate had a length other than 64 UTF-8 bytes.
    ///
    /// `got` is the actual byte length. The spec ABNF
    /// (`64( %x30-39 / %x61-66 )`) is fixed-length and admits no
    /// other length.
    ///
    /// Per-variant `#[non_exhaustive]` so future field additions
    /// (e.g. `expected: usize` once that grows beyond a single
    /// constant 64) remain semver-additive.
    #[non_exhaustive]
    WrongLength {
        /// The candidate's actual byte length.
        got: usize,
    },
    /// The candidate contained a byte outside the lowercase-hex set
    /// `[0-9 a-f]` at the given 0-based position.
    ///
    /// Uppercase hex is intentionally rejected — the spec ABNF
    /// `%x61-66` is the lowercase subset only.
    ///
    /// Per-variant `#[non_exhaustive]` so future field additions
    /// (e.g. the offending byte value — see related finding) remain
    /// semver-additive.
    #[non_exhaustive]
    NonHexLowercase {
        /// 0-based byte index of the first offending character.
        at: usize,
    },
}

impl fmt::Display for Sha256DigestErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongLength { got } => {
                write!(
                    f,
                    "sha256 digest must be exactly 64 lowercase hex chars (got {got})"
                )
            }
            Self::NonHexLowercase { at } => {
                write!(
                    f,
                    "sha256 digest contains a non-lowercase-hex character at byte {at}"
                )
            }
        }
    }
}

/// Parse error produced by [`Sha256::from_hex`] and the [`Sha256`]
/// `Deserialize` impl.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Sha256DigestError {
    /// Specific reason the candidate string failed to parse.
    pub kind: Sha256DigestErrorKind,
}

impl fmt::Display for Sha256DigestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

impl std::error::Error for Sha256DigestError {}

/// SHA-256 digest carried on the JMAP wire as a 64-character
/// lowercase hex string (draft-atwood-jmap-cid-00 §2).
///
/// # Construction
///
/// All four ABNF-validating paths share the same parse logic from
/// [`Sha256::from_hex`]:
///
/// - [`Sha256::from_hex`] — validate a `&str` candidate.
/// - [`TryFrom<&str>`](Sha256#impl-TryFrom<%26str>-for-Sha256) /
///   [`TryFrom<String>`](Sha256#impl-TryFrom<String>-for-Sha256) —
///   validate via `try_into()`.
/// - [`FromStr::from_str`](std::str::FromStr) — validate via
///   `.parse::<Sha256>()`.
/// - `serde::Deserialize` — validates via `TryFrom<String>` per
///   the `#[serde(try_from = "String")]` attribute.
///
/// One infallible constructor:
///
/// - [`Sha256::from_raw_digest`] — format 32 raw digest bytes
///   into the canonical lowercase-hex string. Named to emphasise
///   that the input is the *output* of a hash function (the raw
///   digest), NOT arbitrary input data to be hashed — this crate
///   carries no hash computation.
///
/// # Wire format
///
/// The bare 64-character hex string — `#[serde(try_from, into)]`
/// drives serialization through the validating `TryFrom<String>` /
/// `From<Sha256> for String` adapters so every deserialize path
/// applies the same ABNF check `Sha256::from_hex` does.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
#[non_exhaustive]
pub struct Sha256(String);

impl Sha256 {
    /// Parse a candidate hex string into a [`Sha256`].
    ///
    /// Returns [`Sha256DigestError`] when the candidate is not
    /// exactly 64 bytes long or contains any byte outside the
    /// lowercase-hex set `[0-9 a-f]`. Errors report position so a
    /// caller can surface a precise diagnostic.
    pub fn from_hex(s: &str) -> Result<Self, Sha256DigestError> {
        Self::validate(s)?;
        Ok(Self(s.to_owned()))
    }

    /// ABNF check (`64( %x30-39 / %x61-66 )`) shared by every
    /// construction path. Returns `Ok(())` if `s` is exactly 64
    /// bytes of lowercase hex.
    fn validate(s: &str) -> Result<(), Sha256DigestError> {
        let bytes = s.as_bytes();
        if bytes.len() != 64 {
            return Err(Sha256DigestError {
                kind: Sha256DigestErrorKind::WrongLength { got: bytes.len() },
            });
        }
        // ABNF `%x30-39 / %x61-66` — '0'..='9' or 'a'..='f'.
        for (i, b) in bytes.iter().enumerate() {
            let ok = b.is_ascii_digit() || (b'a'..=b'f').contains(b);
            if !ok {
                return Err(Sha256DigestError {
                    kind: Sha256DigestErrorKind::NonHexLowercase { at: i },
                });
            }
        }
        Ok(())
    }

    /// Format 32 raw digest bytes as a canonical lowercase-hex
    /// [`Sha256`]. Infallible because every byte produces two
    /// lowercase-hex nibbles by construction.
    ///
    /// The input is the **output of a SHA-256 hash function** —
    /// e.g. `sha2::Sha256::digest(data).into()` — not the data to
    /// be hashed. This crate intentionally carries no hash
    /// computation; the name `from_raw_digest` (rather than the
    /// more familiar `from_bytes`) is chosen to make the
    /// input-vs-output distinction unambiguous at call sites.
    ///
    /// # Byte ordering
    ///
    /// The 32 input bytes are taken in the canonical
    /// **FIPS 180-4 SHA-256 output order**: the most significant
    /// byte of the digest first. `b[0]` occupies positions 0..=1
    /// of the resulting hex string and `b[31]` occupies positions
    /// 62..=63. This matches the output of `sha2::Sha256::digest`,
    /// `ring::digest::digest(&SHA256, ...)`, and
    /// `openssl::sha::sha256`. Consumers feeding digest output
    /// from a non-standard source (HSM in non-standard endianness,
    /// pre-reversed archive format) must reorder bytes before
    /// calling this constructor or the wire value will be wrong.
    pub fn from_raw_digest(b: &[u8; 32]) -> Self {
        // 32 bytes → 64 hex chars. Pre-size the buffer to avoid
        // reallocations.
        let mut out = String::with_capacity(64);
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for byte in b {
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
        }
        Self(out)
    }

    /// Borrow the inner 64-character lowercase-hex string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the value and return the inner `String`.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for Sha256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for Sha256 {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Sha256 {
    type Error = Sha256DigestError;
    /// Validate the owned `String` against the ABNF and **move** it
    /// into the [`Sha256`] on success — no second allocation. This
    /// is the path `#[serde(try_from = "String")]` takes, so every
    /// `serde_json::from_str::<Sha256>(...)` deserialize avoids the
    /// double-alloc that `from_hex(&s)` would incur.
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::validate(&s)?;
        Ok(Self(s))
    }
}

impl TryFrom<&str> for Sha256 {
    type Error = Sha256DigestError;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::from_hex(s)
    }
}

impl std::str::FromStr for Sha256 {
    type Err = Sha256DigestError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_hex(s)
    }
}

impl From<Sha256> for String {
    fn from(d: Sha256) -> Self {
        d.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ABNF-positive: a canonical lowercase-hex digest round-trips
    // bit-for-bit through Serialize/Deserialize and `from_hex`.
    const VALID: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn from_hex_accepts_valid_lowercase_64_chars() {
        let d = Sha256::from_hex(VALID).expect("valid digest");
        assert_eq!(d.as_str(), VALID);
        assert_eq!(format!("{d}"), VALID);
    }

    #[test]
    fn from_hex_rejects_uppercase() {
        // Same digest, uppercased — the ABNF is lowercase-only.
        let upper = VALID.to_ascii_uppercase();
        let err = Sha256::from_hex(&upper).expect_err("uppercase rejected");
        match err.kind {
            Sha256DigestErrorKind::NonHexLowercase { at: 0 } => {}
            other => panic!("expected NonHexLowercase {{ at: 0 }}, got {other:?}"),
        }
    }

    #[test]
    fn from_hex_rejects_uppercase_mid_string() {
        // First 31 chars lowercase, byte at index 31 uppercase 'A',
        // remainder lowercase — exercises the position-tracking
        // branch of the validator.
        let mut s = String::from(&VALID[..31]);
        s.push('A');
        s.push_str(&VALID[32..]);
        let err = Sha256::from_hex(&s).expect_err("uppercase mid-string rejected");
        match err.kind {
            Sha256DigestErrorKind::NonHexLowercase { at: 31 } => {}
            other => panic!("expected NonHexLowercase {{ at: 31 }}, got {other:?}"),
        }
    }

    #[test]
    fn from_hex_rejects_short_length() {
        let s = &VALID[..63];
        let err = Sha256::from_hex(s).expect_err("63 chars rejected");
        assert_eq!(err.kind, Sha256DigestErrorKind::WrongLength { got: 63 });
    }

    #[test]
    fn from_hex_rejects_long_length() {
        let mut s = String::from(VALID);
        s.push('0');
        let err = Sha256::from_hex(&s).expect_err("65 chars rejected");
        assert_eq!(err.kind, Sha256DigestErrorKind::WrongLength { got: 65 });
    }

    #[test]
    fn from_hex_rejects_empty() {
        let err = Sha256::from_hex("").expect_err("empty rejected");
        assert_eq!(err.kind, Sha256DigestErrorKind::WrongLength { got: 0 });
    }

    #[test]
    fn from_hex_rejects_non_hex_character() {
        // 63 valid chars then a non-hex 'g' at index 63.
        let mut s = String::from(&VALID[..63]);
        s.push('g');
        let err = Sha256::from_hex(&s).expect_err("non-hex 'g' rejected");
        assert_eq!(err.kind, Sha256DigestErrorKind::NonHexLowercase { at: 63 });
    }

    #[test]
    fn from_raw_digest_formats_canonical_lowercase_hex() {
        // SHA-256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        // (NIST FIPS 180-4 published vector — independent oracle, not
        // derived from this crate).
        let bytes: [u8; 32] = [
            0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
            0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
            0x78, 0x52, 0xb8, 0x55,
        ];
        let d = Sha256::from_raw_digest(&bytes);
        assert_eq!(d.as_str(), VALID);
    }

    #[test]
    fn from_raw_digest_deadbeef_pattern() {
        // First four bytes 0xde 0xad 0xbe 0xef, rest zero — verifies
        // nibble ordering and lowercase-hex character set.
        let mut bytes = [0u8; 32];
        bytes[0] = 0xde;
        bytes[1] = 0xad;
        bytes[2] = 0xbe;
        bytes[3] = 0xef;
        let d = Sha256::from_raw_digest(&bytes);
        assert!(d.as_str().starts_with("deadbeef"));
        assert_eq!(d.as_str().len(), 64);
        // All trailing nibbles should be '0' since the bytes are zero.
        for c in d.as_str().chars().skip(8) {
            assert_eq!(c, '0');
        }
    }

    #[test]
    fn serialize_emits_bare_hex_string() {
        let d = Sha256::from_hex(VALID).unwrap();
        let json = serde_json::to_string(&d).unwrap();
        // Bare string, double-quoted, no wrapper object.
        assert_eq!(json, format!("\"{VALID}\""));
    }

    #[test]
    fn deserialize_accepts_valid_hex_string() {
        let json = format!("\"{VALID}\"");
        let d: Sha256 = serde_json::from_str(&json).unwrap();
        assert_eq!(d.as_str(), VALID);
    }

    #[test]
    fn deserialize_rejects_uppercase() {
        let json = format!("\"{}\"", VALID.to_ascii_uppercase());
        let err = serde_json::from_str::<Sha256>(&json)
            .expect_err("uppercase digest rejected at deserialize");
        let msg = err.to_string();
        assert!(
            msg.contains("non-lowercase-hex"),
            "expected lowercase-hex error, got: {msg}"
        );
    }

    #[test]
    fn deserialize_rejects_wrong_length() {
        let json = "\"abc\"";
        let err =
            serde_json::from_str::<Sha256>(json).expect_err("short digest rejected at deserialize");
        let msg = err.to_string();
        assert!(
            msg.contains("64") && msg.contains("got 3"),
            "expected wrong-length error mentioning 64 and got 3, got: {msg}"
        );
    }

    #[test]
    fn round_trip_through_json_value() {
        // Round-trip via serde_json::Value to exercise the same path
        // a Blob upload response would take.
        let d = Sha256::from_hex(VALID).unwrap();
        let v: serde_json::Value = serde_json::to_value(&d).unwrap();
        assert_eq!(v, serde_json::Value::String(VALID.to_string()));
        let d2: Sha256 = serde_json::from_value(v).unwrap();
        assert_eq!(d, d2);
    }

    #[test]
    fn into_inner_yields_owned_string() {
        let d = Sha256::from_hex(VALID).unwrap();
        let s: String = d.into_inner();
        assert_eq!(s, VALID);
    }

    #[test]
    fn as_ref_str_borrows_inner() {
        let d = Sha256::from_hex(VALID).unwrap();
        let s: &str = d.as_ref();
        assert_eq!(s, VALID);
    }

    #[test]
    fn from_str_works() {
        let d: Sha256 = VALID.parse().unwrap();
        assert_eq!(d.as_str(), VALID);
    }

    #[test]
    fn try_from_string_moves_buffer_not_clones() {
        // TryFrom<String> should MOVE the owned String into the
        // Sha256, not validate then clone. Verify by checking that
        // the resulting Sha256's underlying byte buffer has the
        // same address as the input String's buffer — pointer
        // identity proves no second allocation occurred.
        let input = VALID.to_owned();
        let input_ptr = input.as_ptr();
        let d: Sha256 = input.try_into().expect("valid digest");
        assert_eq!(
            d.as_str().as_ptr(),
            input_ptr,
            "TryFrom<String> must move the owned buffer; pointer mismatch \
             indicates a re-allocation"
        );
    }

    #[test]
    fn try_from_string_validates() {
        let d: Sha256 = VALID.to_string().try_into().unwrap();
        assert_eq!(d.as_str(), VALID);
        let err: Result<Sha256, _> = "bogus".to_string().try_into();
        assert!(err.is_err());
    }

    #[test]
    fn error_display_includes_position() {
        let err = Sha256DigestError {
            kind: Sha256DigestErrorKind::NonHexLowercase { at: 17 },
        };
        assert!(err.to_string().contains("byte 17"));
        let err = Sha256DigestError {
            kind: Sha256DigestErrorKind::WrongLength { got: 65 },
        };
        assert!(err.to_string().contains("65"));
    }
}
