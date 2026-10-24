//! CIDv1 conversion for [`Sha256`], per draft-atwood-jmap-cid-00
//! §Mapping to CIDv1 (informative appendix).
//!
//! Spec text:
//!
//! > A `sha256` value converts losslessly to a CIDv1 as defined by
//! > {{MULTIFORMATS-CID}}, using the `raw` codec and the `sha2-256`
//! > multihash.
//!
//! The binary CIDv1 is the four prefix bytes `0x01 0x55 0x12 0x20`
//! (CID version 1, multicodec `raw`, multihash `sha2-256`, digest
//! length 32) followed by the 32 digest bytes. The string form is the
//! multibase prefix `b` followed by the binary CIDv1 as lowercase
//! RFC 4648 base32 without padding.
//!
//! Only this one CID shape is accepted on the reverse path: "The
//! reverse conversion checks the four prefix bytes and hex-encodes
//! the remaining 32 bytes." Any other version, codec, hash function or
//! multibase is rejected with [`CidV1Error`].

use crate::Sha256;
use std::fmt;

/// The four bytes that open every CIDv1 a [`Sha256`] maps to:
/// version 1, multicodec `raw`, multihash `sha2-256`, length 32.
const PREFIX: [u8; 4] = [0x01, 0x55, 0x12, 0x20];

/// Length of the binary CIDv1: 4 prefix bytes plus a 32-byte digest.
const CIDV1_LEN: usize = PREFIX.len() + 32;

/// RFC 4648 §6 base32 alphabet, lowercased as multibase `b` requires.
const BASE32_LOWER: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// Error from [`Sha256::from_cidv1`] and [`Sha256::from_cidv1_bytes`].
///
/// `#[non_exhaustive]` at the type level and per variant, so new
/// variants and new fields stay semver-additive.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CidV1Error {
    /// The string does not start with the multibase prefix `b`
    /// (lowercase base32, no padding). Other multibase encodings,
    /// including `B` (uppercase base32), are not accepted.
    #[non_exhaustive]
    UnsupportedMultibase,
    /// A byte after the multibase prefix is outside the lowercase
    /// base32 alphabet `a-z 2-7`.
    #[non_exhaustive]
    InvalidBase32 {
        /// 0-based byte index in the input string, saturated at
        /// `u32::MAX`.
        at: u32,
        /// The offending byte.
        byte: u8,
    },
    /// The base32 text has a length no byte string encodes to, or its
    /// unused trailing bits are not zero, so it is not the canonical
    /// encoding of any binary CID.
    #[non_exhaustive]
    NonCanonicalBase32,
    /// The binary CID is not 36 bytes long.
    #[non_exhaustive]
    WrongLength {
        /// The actual byte length, saturated at `u32::MAX`.
        got: u32,
    },
    /// The first four bytes are not `0x01 0x55 0x12 0x20`, so the CID
    /// is not a version 1, `raw`, `sha2-256` CID.
    #[non_exhaustive]
    UnsupportedPrefix {
        /// The four bytes found.
        got: [u8; 4],
    },
}

impl fmt::Display for CidV1Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedMultibase => {
                f.write_str("CIDv1 string must start with the multibase prefix 'b'")
            }
            Self::InvalidBase32 { at, byte } => write!(
                f,
                "CIDv1 string contains a non-base32 byte 0x{byte:02x} at byte {at}"
            ),
            Self::NonCanonicalBase32 => f.write_str("CIDv1 string is not canonical base32"),
            Self::WrongLength { got } => {
                write!(f, "binary CIDv1 must be {CIDV1_LEN} bytes (got {got})")
            }
            Self::UnsupportedPrefix { got } => write!(
                f,
                "CIDv1 prefix must be 01 55 12 20 (got {:02x} {:02x} {:02x} {:02x})",
                got[0], got[1], got[2], got[3]
            ),
        }
    }
}

impl std::error::Error for CidV1Error {}

impl Sha256 {
    /// The binary CIDv1 for this digest: `0x01 0x55 0x12 0x20`
    /// followed by the 32 digest bytes.
    pub fn to_cidv1_bytes(&self) -> [u8; CIDV1_LEN] {
        let mut out = [0u8; CIDV1_LEN];
        out[..PREFIX.len()].copy_from_slice(&PREFIX);
        out[PREFIX.len()..].copy_from_slice(&self.to_bytes());
        out
    }

    /// The CIDv1 string form for this digest: `b` followed by the
    /// binary CIDv1 as lowercase base32 without padding.
    pub fn to_cidv1(&self) -> String {
        let bytes = self.to_cidv1_bytes();
        let mut out = String::with_capacity(1 + (bytes.len() * 8).div_ceil(5));
        out.push('b');
        let mut acc: u32 = 0;
        let mut bits = 0;
        for &b in &bytes {
            acc = (acc << 8) | u32::from(b);
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                out.push(char::from(BASE32_LOWER[((acc >> bits) & 0x1f) as usize]));
            }
        }
        if bits > 0 {
            out.push(char::from(
                BASE32_LOWER[((acc << (5 - bits)) & 0x1f) as usize],
            ));
        }
        out
    }

    /// Parse a binary CIDv1 back into its [`Sha256`].
    ///
    /// Rejects anything that is not exactly 36 bytes starting with
    /// `0x01 0x55 0x12 0x20`.
    pub fn from_cidv1_bytes(bytes: &[u8]) -> Result<Self, CidV1Error> {
        let digest: &[u8; 32] = bytes
            .strip_prefix(&PREFIX)
            .and_then(|rest| rest.try_into().ok())
            .ok_or_else(|| match bytes.first_chunk::<4>() {
                Some(got) if *got != PREFIX => CidV1Error::UnsupportedPrefix { got: *got },
                _ => CidV1Error::WrongLength {
                    got: u32::try_from(bytes.len()).unwrap_or(u32::MAX),
                },
            })?;
        Ok(Self::from(digest))
    }

    /// Parse a CIDv1 string (`b` + lowercase unpadded base32) back
    /// into its [`Sha256`].
    pub fn from_cidv1(s: &str) -> Result<Self, CidV1Error> {
        let text = s
            .as_bytes()
            .strip_prefix(b"b")
            .ok_or(CidV1Error::UnsupportedMultibase)?;
        let mut bytes = Vec::with_capacity(text.len() * 5 / 8);
        let mut acc: u32 = 0;
        let mut bits = 0;
        for (i, &c) in text.iter().enumerate() {
            let value = match c {
                b'a'..=b'z' => c - b'a',
                b'2'..=b'7' => c - b'2' + 26,
                _ => {
                    return Err(CidV1Error::InvalidBase32 {
                        at: u32::try_from(i + 1).unwrap_or(u32::MAX),
                        byte: c,
                    });
                }
            };
            acc = ((acc << 5) | u32::from(value)) & 0xff_ff;
            bits += 5;
            if bits >= 8 {
                bits -= 8;
                bytes.push((acc >> bits) as u8);
            }
        }
        // RFC 4648 §6: leftover bits are padding and must be zero; five
        // or more leftover bits means a character encoded no data.
        if bits >= 5 || acc & ((1 << bits) - 1) != 0 {
            return Err(CidV1Error::NonCanonicalBase32);
        }
        Self::from_cidv1_bytes(&bytes)
    }
}
