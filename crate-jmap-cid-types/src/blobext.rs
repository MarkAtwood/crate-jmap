//! Base64 conversion for [`Sha256`], for comparison with the RFC 9404
//! (JMAP Blob Management Extensions) `digest:sha-256` property.
//!
//! draft-atwood-jmap-cid-00 §Relationship to JMAP Blob Management
//! Extensions says the CID `sha256` field "uses lowercase hex encoding
//! rather than base64", and that clients may cross-validate the two
//! "provided they convert encodings before comparing". RFC 9404 §4.1
//! defines `digest:<algorithm>` as "The base64 encoding of the digest",
//! with base64 as specified in RFC 4648: the §4 alphabet with `=`
//! padding. A 32-byte SHA-256 digest is always 44 base64 characters.
//!
//! The reverse path accepts only canonical padded base64. Missing
//! padding, the §5 URL-safe alphabet, embedded whitespace and non-zero
//! trailing bits are all rejected with [`Sha256Base64Error`].

use crate::Sha256;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use std::fmt;

/// Error from [`Sha256::from_base64`].
///
/// `#[non_exhaustive]` at the type level and per variant, so new
/// variants and new fields stay semver-additive.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sha256Base64Error {
    /// The string is not canonical padded RFC 4648 §4 base64.
    #[non_exhaustive]
    InvalidBase64,
    /// The string decodes to something other than 32 bytes.
    #[non_exhaustive]
    WrongLength {
        /// The decoded byte length, saturated at `u32::MAX`.
        got: u32,
    },
}

impl fmt::Display for Sha256Base64Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBase64 => f.write_str("SHA-256 digest is not canonical padded base64"),
            Self::WrongLength { got } => {
                write!(
                    f,
                    "base64 SHA-256 digest must decode to 32 bytes (got {got})"
                )
            }
        }
    }
}

impl std::error::Error for Sha256Base64Error {}

impl Sha256 {
    /// The digest as padded RFC 4648 §4 base64, the encoding of the
    /// RFC 9404 `digest:sha-256` property.
    pub fn to_base64(&self) -> String {
        STANDARD.encode(self.to_bytes())
    }

    /// Parse a padded RFC 4648 §4 base64 digest, such as an RFC 9404
    /// `digest:sha-256` value, into its [`Sha256`].
    pub fn from_base64(s: &str) -> Result<Self, Sha256Base64Error> {
        let bytes = STANDARD
            .decode(s)
            .map_err(|_| Sha256Base64Error::InvalidBase64)?;
        let digest: [u8; 32] =
            bytes
                .as_slice()
                .try_into()
                .map_err(|_| Sha256Base64Error::WrongLength {
                    got: u32::try_from(bytes.len()).unwrap_or(u32::MAX),
                })?;
        Ok(Self::from(digest))
    }
}
