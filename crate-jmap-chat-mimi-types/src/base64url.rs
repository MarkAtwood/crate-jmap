//! `Base64Url` — the byte-string wire shape of
//! draft-atwood-jmap-chat-mimi-00 §Conventions and Definitions.
//!
//! Spec text:
//!
//! > Every byte-string property defined in {{capability}} through
//! > {{mls-messages}} is the unpadded base64url encoding
//! > ({{RFC4648, Section 5}}) of the exact octets of the named MLS
//! > {{RFC9420}} or MIMI {{MIMI-PROTOCOL}} structure in the TLS
//! > presentation language encoding those documents use. Servers MUST
//! > NOT re-encode these octets, because clients sign some of them and
//! > verify signatures over others.
//!
//! The type validates the string and keeps it verbatim. It carries no
//! decoder: consumers that parse the MLS structure decode with their own
//! base64 and TLS-codec stack.

use serde::{Deserialize, Serialize, Serializer};
use std::fmt;

/// Parse error produced by [`Base64Url`] construction and deserialize.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Base64UrlError {
    /// A byte outside the RFC 4648 §5 alphabet `[A-Za-z0-9_-]`. Padding
    /// `=` lands here too, because the draft requires the unpadded form.
    #[non_exhaustive]
    InvalidByte {
        /// 0-based byte index, saturated at `u32::MAX`.
        at: u32,
        /// The offending byte.
        byte: u8,
    },
    /// The length is 1 more than a multiple of 4, which no octet string
    /// encodes to.
    #[non_exhaustive]
    InvalidLength {
        /// The candidate's byte length, saturated at `u32::MAX`.
        len: u32,
    },
    /// The final character has non-zero pad bits, so the string is not
    /// the encoding of any octet string (RFC 4648 §3.5).
    NonCanonical,
}

impl fmt::Display for Base64UrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidByte { at, byte } => write!(
                f,
                "unpadded base64url contains invalid byte 0x{byte:02x} at byte {at}"
            ),
            Self::InvalidLength { len } => {
                write!(f, "unpadded base64url cannot have length {len}")
            }
            Self::NonCanonical => f.write_str("unpadded base64url has non-zero pad bits"),
        }
    }
}

impl std::error::Error for Base64UrlError {}

/// An unpadded base64url string (RFC 4648 §5) holding the exact
/// TLS-encoded octets of an MLS or MIMI structure.
///
/// Construction rejects any string that is not the canonical encoding of
/// some octet string: bytes outside `[A-Za-z0-9_-]` (including `=`), a
/// length of `4n + 1`, and non-zero pad bits. Two values are therefore
/// equal exactly when they encode the same octets.
///
/// # Example
///
/// ```
/// use jmap_chat_mimi_types::Base64Url;
///
/// // draft-atwood-jmap-chat-mimi-00 §Conventions: the KeyPackageRef
/// // whose hash value is the octets 0x00 through 0x1f.
/// let r: Base64Url = "IAABAgMEBQYHCAkKCwwNDg8QERITFBUWFxgZGhscHR4f".parse()?;
/// assert_eq!(r.as_str(), "IAABAgMEBQYHCAkKCwwNDg8QERITFBUWFxgZGhscHR4f");
/// assert!("AA==".parse::<Base64Url>().is_err());
/// # Ok::<(), jmap_chat_mimi_types::Base64UrlError>(())
/// ```
///
/// # Allocation bounds
///
/// Deserialize goes through `TryFrom<String>`, so the whole string is
/// allocated before validation. Bound body sizes at the transport layer,
/// as for `jmap_cid_types::Sha256`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct Base64Url(String);

impl Serialize for Base64Url {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

impl Base64Url {
    /// Borrow the validated string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn validate(s: &str) -> Result<(), Base64UrlError> {
        let bytes = s.as_bytes();
        let sextet = |b: u8| match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        };
        let mut last = 0;
        for (i, &b) in bytes.iter().enumerate() {
            last = sextet(b).ok_or(Base64UrlError::InvalidByte {
                at: u32::try_from(i).unwrap_or(u32::MAX),
                byte: b,
            })?;
        }
        // A trailing group of 2 or 3 characters carries 4 or 2 pad bits.
        let pad_mask = match bytes.len() % 4 {
            0 => 0,
            1 => {
                return Err(Base64UrlError::InvalidLength {
                    len: u32::try_from(bytes.len()).unwrap_or(u32::MAX),
                })
            }
            2 => 0b1111,
            _ => 0b11,
        };
        if last & pad_mask != 0 {
            return Err(Base64UrlError::NonCanonical);
        }
        Ok(())
    }
}

impl TryFrom<String> for Base64Url {
    type Error = Base64UrlError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::validate(&s)?;
        Ok(Self(s))
    }
}

impl std::str::FromStr for Base64Url {
    type Err = Base64UrlError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::validate(s)?;
        Ok(Self(s.to_owned()))
    }
}

impl AsRef<str> for Base64Url {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Base64Url {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<Base64Url, Base64UrlError> {
        s.parse()
    }

    #[test]
    fn draft_keypackageref_example_accepted() {
        // Oracle: draft §Conventions: "its `keyPackageRef` is
        // `\"IAABAgMEBQYHCAkKCwwNDg8QERITFBUWFxgZGhscHR4f\"`".
        let s = "IAABAgMEBQYHCAkKCwwNDg8QERITFBUWFxgZGhscHR4f";
        let v = parse(s).unwrap();
        assert_eq!(serde_json::to_string(&v).unwrap(), format!("\"{s}\""));
        let back: Base64Url = serde_json::from_str(&format!("\"{s}\"")).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn whole_alphabet_accepted() {
        // Oracle: RFC 4648 §5 Table 2, values 0..63 in order, with
        // 62 = '-' and 63 = '_'. 64 characters = 48 octets, no pad bits.
        let s = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        assert!(parse(s).is_ok());
    }

    #[test]
    fn empty_string_accepted() {
        // The encoding of zero octets is the empty string.
        assert!(parse("").is_ok());
    }

    #[test]
    fn padding_and_standard_alphabet_rejected() {
        assert_eq!(
            parse("AA=="),
            Err(Base64UrlError::InvalidByte { at: 2, byte: b'=' })
        );
        assert_eq!(
            parse("ab+c"),
            Err(Base64UrlError::InvalidByte { at: 2, byte: b'+' })
        );
        assert_eq!(
            parse("ab/c"),
            Err(Base64UrlError::InvalidByte { at: 2, byte: b'/' })
        );
    }

    #[test]
    fn length_four_n_plus_one_rejected() {
        assert_eq!(
            parse("AAAAA"),
            Err(Base64UrlError::InvalidLength { len: 5 })
        );
    }

    #[test]
    fn non_zero_pad_bits_rejected() {
        // Hand-computed: "AA" is 000000 000000 -> octet 0x00 with 4 zero
        // pad bits; "AB" sets the last pad bit. "AAE" is 0x00 0x01 with
        // 'E' = 000100, pad bits 00; "AAB" has pad bits 01.
        assert!(parse("AA").is_ok());
        assert_eq!(parse("AB"), Err(Base64UrlError::NonCanonical));
        assert!(parse("AAE").is_ok());
        assert_eq!(parse("AAB"), Err(Base64UrlError::NonCanonical));
    }

    #[test]
    fn deserialize_rejects_invalid() {
        assert!(serde_json::from_str::<Base64Url>("\"AA==\"").is_err());
    }
}
