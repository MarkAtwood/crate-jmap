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
//! bytes via [`From`]`<[u8; 32]>` / [`From`]`<&[u8; 32]>` for
//! [`Sha256`] to format the wire value.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Parse error produced by [`Sha256::from_hex`] and the [`Sha256`]
/// `Deserialize` impl.
///
/// The enum is `#[non_exhaustive]` at the type level and every
/// variant is `#[non_exhaustive]` so future field additions and
/// variant additions both remain semver-additive (bd:JMAP-sf5h.21
/// ratified the single-tier shape — no wrapper struct is needed
/// because no extra context lives on the wrapper).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Sha256DigestError {
    /// The candidate had a length other than 64 UTF-8 bytes.
    ///
    /// `got` is the actual byte length. The spec ABNF
    /// (`64( %x30-39 / %x61-66 )`) is fixed-length and admits no
    /// other length.
    ///
    /// `got` is `u32` (saturating, capped at `u32::MAX`) rather
    /// than `usize` to avoid leaking a platform-dependent
    /// integer width into the public error contract — the value
    /// is realistically bounded by the input string length and any
    /// candidate over 4 GiB would have been rejected earlier in the
    /// parsing pipeline.
    ///
    /// Per-variant `#[non_exhaustive]` so future field additions
    /// remain semver-additive.
    #[non_exhaustive]
    WrongLength {
        /// The candidate's actual byte length, saturated at
        /// `u32::MAX` for inputs larger than 4 GiB.
        got: u32,
    },
    /// The candidate contained a byte outside the lowercase-hex set
    /// `[0-9 a-f]` at the given 0-based position.
    ///
    /// Uppercase hex is intentionally rejected — the spec ABNF
    /// `%x61-66` is the lowercase subset only.
    ///
    /// `at` is `u32` (saturating, capped at `u32::MAX`) rather
    /// than `usize`; `byte` is the offending UTF-8 byte itself so
    /// the diagnostic can report `0x41 ('A')` for an uppercase
    /// candidate without the caller having to re-index the input.
    ///
    /// Per-variant `#[non_exhaustive]` so future field additions
    /// remain semver-additive.
    #[non_exhaustive]
    NonHexLowercase {
        /// 0-based byte index of the first offending character,
        /// saturated at `u32::MAX` for inputs larger than 4 GiB.
        at: u32,
        /// The offending UTF-8 byte value at `at`.
        byte: u8,
    },
}

impl fmt::Display for Sha256DigestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongLength { got } => {
                write!(
                    f,
                    "sha256 digest must be exactly 64 lowercase hex chars (got {got})"
                )
            }
            Self::NonHexLowercase { at, byte } => {
                write!(
                    f,
                    "sha256 digest contains a non-lowercase-hex byte 0x{byte:02x} at byte {at}"
                )
            }
        }
    }
}

impl std::error::Error for Sha256DigestError {}

/// SHA-256 digest carried on the JMAP wire as a 64-character
/// lowercase hex string (draft-atwood-jmap-cid-00 §2).
///
/// # Example
///
/// ```
/// use jmap_cid_types::Sha256;
///
/// // The SHA-256 of the empty string (FIPS 180-4 published vector).
/// let hex = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
/// let d: Sha256 = hex.parse()?;
/// assert_eq!(d.as_str(), hex);
///
/// // Round-trips bit-for-bit through serde as a bare JSON string.
/// let json = serde_json::to_string(&d)?;
/// assert_eq!(json, format!("\"{hex}\""));
/// let d2: Sha256 = serde_json::from_str(&json)?;
/// assert_eq!(d, d2);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
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
/// One infallible conversion:
///
/// - [`From`]`<[u8; 32]>` / [`From`]`<&[u8; 32]>` for [`Sha256`] —
///   format 32 raw digest bytes into the canonical lowercase-hex
///   string. Infallible because every byte produces two
///   lowercase-hex nibbles by construction. The input is the
///   *output* of a hash function (the raw digest), NOT arbitrary
///   input data to be hashed — this crate carries no hash
///   computation. See the [`From<&[u8; 32]>`](#impl-From%3C%26%5Bu8;+32%5D%3E-for-Sha256)
///   impl for byte-ordering details.
///
/// # Wire format
///
/// The bare 64-character hex string — `#[serde(try_from, into)]`
/// drives serialization through the validating `TryFrom<String>` /
/// `From<Sha256> for String` adapters so every deserialize path
/// applies the same ABNF check `Sha256::from_hex` does.
///
/// # Allocation bounds
///
/// **`Sha256` does not bound allocation before validation.** The
/// `#[serde(try_from = "String")]` adapter routes every deserialize
/// path through `TryFrom<String>`, which means `serde_json` (and any
/// other serde data format) **materialises the full input string
/// before** the 64-byte ABNF cap rejects oversize values. A hostile
/// JMAP peer responding with `{"sha256": "aaaa…"}` where the payload
/// is gigabytes of `'a'` will force the consumer to allocate
/// `O(payload-length)` bytes per deserialize attempt before the
/// `WrongLength` error fires.
///
/// Consumers deserialising `Sha256` (directly or transitively, e.g.
/// via [`jmap-base-client`]'s `BlobUploadResponse.sha256` field) from
/// an untrusted or partially-trusted JMAP peer MUST enforce a
/// **body-size limit at the transport layer**:
///
/// - `reqwest::Response::bytes()` does not bound by default; use
///   `Response::bytes_stream()` plus a wrapping `take(N)` byte cap,
///   or check `Response::content_length()` against a policy maximum
///   before reading.
/// - `tokio_tungstenite` WebSocket frames default to a 64 MiB limit
///   per frame (`WebSocketConfig::max_frame_size`); JMAP push frames
///   carrying a digest field do not need 64 MiB and the limit can be
///   tightened.
/// - Hand-rolled HTTP clients reading a `Body` MUST cap with a
///   `take(N)` adapter before calling `serde_json::from_reader`.
///
/// In-scope threats:
///
/// - A JMAP client connected to a malicious or compromised JMAP
///   server. The server controls the response body.
/// - A JMAP middleware (federation peer, mirror, cache) processing
///   responses from a peer it does not fully trust.
/// - A JMAP client behind a proxy that rewrites response bodies
///   (less likely on TLS, but possible on metadata or via injection).
///
/// Out of scope: a trusted server returning oversize bodies by
/// accident. Transport-layer bounding handles both cases uniformly.
///
/// This crate intentionally does not switch to a custom
/// `Deserialize` impl with `Visitor::visit_str` length pre-check —
/// the workspace canonical pattern keeps validation centralised in
/// `Sha256::from_hex` so the ABNF check is a single source of truth
/// (see bd:JMAP-sf5h.9 for the decision record). Self-bounding at
/// the type level would shift validation responsibility into the
/// `Deserialize` impl and away from `from_hex`; the workspace prefers
/// transport-layer bounding over per-type bounding because the
/// transport bound applies uniformly to **every** field on a
/// hostile response, not just `sha256` ones.
///
/// [`jmap-base-client`]: https://docs.rs/jmap-base-client
///
/// # Equality and threat model
///
/// `PartialEq` and `Eq` on `Sha256` inherit the standard
/// **variable-time** `String` compare — short-circuits on the
/// first byte mismatch. This is appropriate for the in-scope
/// JMAP CID use case (`draft-atwood-jmap-cid-00`): the digest is
/// the SHA-256 of a **public blob** broadcast to every party
/// with read access, so there is no secret comparison and the
/// timing channel reveals nothing an attacker does not already
/// have.
///
/// Consumers repurposing `Sha256` for **secret-derived
/// comparisons** — MAC-style verification, commitment opening,
/// capability tokens — MUST use a constant-time compare via
/// [`subtle::ConstantTimeEq`] on the underlying bytes. Extract
/// via [`Sha256::as_str`]`().as_bytes()`. The variable-time
/// short-circuit otherwise leaks the prefix length of a matching
/// digest to a remote attacker on the classic Bleichenbacher-class
/// timing pattern.
///
/// This crate intentionally does not ship a constant-time
/// comparison helper because the workspace policy is that
/// MAC/HMAC-shaped verification belongs in a separate type
/// (`MacTag`, `SecretDigest`) so the choice of constant-time
/// equality is part of the type contract, not a per-call-site
/// opt-in on a wire-format type.
///
/// [`subtle::ConstantTimeEq`]: https://docs.rs/subtle/latest/subtle/trait.ConstantTimeEq.html
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
            return Err(Sha256DigestError::WrongLength {
                got: u32::try_from(bytes.len()).unwrap_or(u32::MAX),
            });
        }
        // ABNF `%x30-39 / %x61-66` — '0'..='9' or 'a'..='f'.
        for (i, b) in bytes.iter().enumerate() {
            let ok = b.is_ascii_digit() || (b'a'..=b'f').contains(b);
            if !ok {
                return Err(Sha256DigestError::NonHexLowercase {
                    // `i` is bounded by the 64-byte length check
                    // above; the cast never saturates here, but
                    // u32::try_from preserves the type-contract
                    // posture if the length check is ever
                    // relaxed.
                    at: u32::try_from(i).unwrap_or(u32::MAX),
                    byte: *b,
                });
            }
        }
        Ok(())
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

impl From<&[u8; 32]> for Sha256 {
    /// Format 32 raw digest bytes as a canonical lowercase-hex
    /// [`Sha256`]. Infallible because every byte produces two
    /// lowercase-hex nibbles by construction.
    ///
    /// The input is the **output of a SHA-256 hash function** —
    /// e.g. `sha2::Sha256::digest(data).into()` — not the data to
    /// be hashed. This crate intentionally carries no hash
    /// computation.
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
    /// the conversion or the wire value will be wrong.
    fn from(b: &[u8; 32]) -> Self {
        use std::fmt::Write as _;
        // 32 bytes → 64 hex chars. Pre-size the buffer to avoid
        // reallocations; the `{:02x}` formatter writes two
        // lowercase-hex nibbles per byte using std's well-tested
        // hex formatter.
        let mut out = String::with_capacity(64);
        for byte in b {
            // write! to a String never fails (the String never
            // returns Err from its fmt::Write impl) — the .expect
            // documents that the only Err path is unreachable.
            write!(out, "{byte:02x}").expect("write! to String is infallible");
        }
        Self(out)
    }
}

impl From<[u8; 32]> for Sha256 {
    /// Ergonomic owned-array conversion. Delegates to
    /// [`From`]`<&[u8; 32]>` for [`Sha256`] — see that impl for
    /// byte-ordering details. Provided so callers with an owned
    /// digest array (e.g. `sha2::Sha256::digest(data).into()`)
    /// can write `Sha256::from(bytes)` or `bytes.into()` without
    /// reaching for `&bytes` at the call site.
    fn from(b: [u8; 32]) -> Self {
        Self::from(&b)
    }
}

// Sibling-of-Id newtype: matches the impl_string_newtype! macro
// surface from `jmap-types/src/id.rs` (PartialEq<str>,
// PartialEq<&str>, Borrow<str>) so `Sha256` reads like every other
// wire-format newtype in the workspace. The infallible From<String>
// / From<&str> impls are deliberately omitted — `Sha256`'s ABNF
// is closed (exactly 64 lowercase hex chars), unlike Id's open
// SAFE-CHAR set, so construction is strictly fallible. See
// bd:JMAP-sf5h.7 for the decision record.
impl PartialEq<str> for Sha256 {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for Sha256 {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl std::borrow::Borrow<str> for Sha256 {
    fn borrow(&self) -> &str {
        &self.0
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
        // The offending byte is the first character of the upper-cased
        // VALID; oracle: 'E' = 0x45 (first char of the SHA-256 zero
        // vector is 'e', upper-cased to 'E').
        let upper = VALID.to_ascii_uppercase();
        let err = Sha256::from_hex(&upper).expect_err("uppercase rejected");
        match err {
            Sha256DigestError::NonHexLowercase { at: 0, byte: 0x45 } => {}
            other => panic!("expected NonHexLowercase {{ at: 0, byte: 0x45 }}, got {other:?}"),
        }
    }

    #[test]
    fn from_hex_rejects_uppercase_mid_string() {
        // First 31 chars lowercase, byte at index 31 uppercase 'A'
        // (0x41), remainder lowercase — exercises the
        // position-tracking branch of the validator.
        let mut s = String::from(&VALID[..31]);
        s.push('A');
        s.push_str(&VALID[32..]);
        let err = Sha256::from_hex(&s).expect_err("uppercase mid-string rejected");
        match err {
            Sha256DigestError::NonHexLowercase { at: 31, byte: 0x41 } => {}
            other => panic!("expected NonHexLowercase {{ at: 31, byte: 0x41 }}, got {other:?}"),
        }
    }

    #[test]
    fn from_hex_rejects_short_length() {
        let s = &VALID[..63];
        let err = Sha256::from_hex(s).expect_err("63 chars rejected");
        assert_eq!(err, Sha256DigestError::WrongLength { got: 63 });
    }

    #[test]
    fn from_hex_rejects_long_length() {
        let mut s = String::from(VALID);
        s.push('0');
        let err = Sha256::from_hex(&s).expect_err("65 chars rejected");
        assert_eq!(err, Sha256DigestError::WrongLength { got: 65 });
    }

    #[test]
    fn from_hex_rejects_empty() {
        let err = Sha256::from_hex("").expect_err("empty rejected");
        assert_eq!(err, Sha256DigestError::WrongLength { got: 0 });
    }

    #[test]
    fn from_hex_rejects_non_hex_character() {
        // 63 valid chars then a non-hex 'g' (0x67) at index 63.
        let mut s = String::from(&VALID[..63]);
        s.push('g');
        let err = Sha256::from_hex(&s).expect_err("non-hex 'g' rejected");
        assert_eq!(
            err,
            Sha256DigestError::NonHexLowercase { at: 63, byte: 0x67 }
        );
    }

    #[test]
    fn from_borrowed_array_formats_canonical_lowercase_hex() {
        // SHA-256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        // (NIST FIPS 180-4 published vector — independent oracle, NOT
        // derived from this crate). The vector is hand-copied into
        // the test rather than computed via sha2::Sha256::digest() at
        // test time, because the latter would close the oracle loop:
        // any nibble-ordering or character-table bug in the
        // From<&[u8; 32]> impl would emit the same wrong digest the
        // test expects. See bd:JMAP-sf5h.8 for the decision record.
        let bytes: [u8; 32] = [
            0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
            0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
            0x78, 0x52, 0xb8, 0x55,
        ];
        let d = Sha256::from(&bytes);
        assert_eq!(d.as_str(), VALID);
    }

    #[test]
    fn from_owned_array_delegates_to_borrowed_path() {
        // The owned-array From<[u8; 32]> impl must produce the same
        // wire string as the borrowed-array From<&[u8; 32]> impl for
        // the same input — they share a single nibble-formatting
        // path. Same FIPS 180-4 vector as the borrowed-path test.
        let bytes: [u8; 32] = [
            0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
            0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
            0x78, 0x52, 0xb8, 0x55,
        ];
        // `.into()` exercises the From<[u8; 32]> path.
        let d: Sha256 = bytes.into();
        assert_eq!(d.as_str(), VALID);
        // And both impls agree.
        assert_eq!(d, Sha256::from(&bytes));
    }

    #[test]
    fn from_array_deadbeef_pattern() {
        // First four bytes 0xde 0xad 0xbe 0xef, rest zero — verifies
        // nibble ordering and lowercase-hex character set.
        let mut bytes = [0u8; 32];
        bytes[0] = 0xde;
        bytes[1] = 0xad;
        bytes[2] = 0xbe;
        bytes[3] = 0xef;
        let d = Sha256::from(&bytes);
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
    fn partial_eq_str_compares_against_string_slice() {
        // Sibling-of-Id pattern (crate-jmap-types/src/id.rs): a wire
        // newtype compares directly against &str without forcing the
        // caller to write .as_str() at the call site.
        let d = Sha256::from_hex(VALID).unwrap();
        assert!(d == *VALID);
        assert!(d != *"deadbeef");
    }

    #[test]
    fn partial_eq_ref_str_compares_against_borrowed_slice() {
        let d = Sha256::from_hex(VALID).unwrap();
        let s: &str = VALID;
        assert!(d == s);
        let other: &str = "0000000000000000000000000000000000000000000000000000000000000000";
        assert!(d != other);
    }

    #[test]
    fn borrow_str_enables_hashmap_lookup_by_str_key() {
        // Without `impl Borrow<str> for Sha256`, HashMap<Sha256, _>::get(&str)
        // does not compile. This test demonstrates the lookup pattern works.
        use std::borrow::Borrow;
        use std::collections::HashMap;

        let d = Sha256::from_hex(VALID).unwrap();
        // Sanity: Borrow<str> yields the same bytes as as_str().
        let borrowed: &str = d.borrow();
        assert_eq!(borrowed, VALID);

        let mut m: HashMap<Sha256, &'static str> = HashMap::new();
        m.insert(d, "value");
        // Look up by &str — this compiles only when Borrow<str> exists.
        assert_eq!(m.get(VALID), Some(&"value"));
    }

    #[test]
    fn error_display_includes_position_and_byte() {
        // Oracle: hand-constructed variant; Display must surface
        // both the byte position and the offending byte value so
        // the diagnostic is actionable without re-indexing the
        // input.
        let err = Sha256DigestError::NonHexLowercase { at: 17, byte: 0x41 };
        let msg = err.to_string();
        assert!(msg.contains("byte 17"), "{msg}");
        assert!(msg.contains("0x41"), "{msg}");
        let err = Sha256DigestError::WrongLength { got: 65 };
        assert!(err.to_string().contains("65"));
    }
}
