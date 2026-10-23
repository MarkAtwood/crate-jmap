//! `did:key` resolution by local derivation (draft-atwood-jmap-chat-did-00
//! §Mandatory DID Methods: "resolution by deterministic local derivation
//! of the DID Document from the public key material encoded in the DID
//! URI itself. No network resolution is performed.").
//!
//! The algorithms are those of the did:key Method (W3C CCG, v0.9):
//! the Decode Public Key Algorithm, the key-length table of the
//! Signature Method Creation Algorithm, and the Document Creation
//! Algorithm.

use std::fmt;

use serde_json::{Value, json};

use crate::Did;

/// The base58-btc alphabet (multibase prefix `z`).
pub(crate) const BASE58_ALPHABET: &[u8; 58] =
    b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

/// Longest multibase value decoded: room for every key in the multicodec
/// table, so an unsupported one (a 96-byte BLS12-381 G2 key is about
/// 135 base58 characters) reports its multicodec.
// ponytail: the cap keeps the quadratic base58 decode cheap on hostile input.
const MAX_MULTIBASE_LEN: usize = 256;

/// A public key type a `did:key` can encode: one row of the did:key
/// Signature Method Creation Algorithm table.
///
/// `#[non_exhaustive]`, so new key types stay semver-additive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum KeyType {
    /// `secp256k1-pub` (multicodec `0xe7`), a 33-byte compressed point.
    Secp256k1,
    /// `x25519-pub` (multicodec `0xec`), a 32-byte Curve25519 key.
    X25519,
    /// `ed25519-pub` (multicodec `0xed`), a 32-byte Ed25519 key.
    Ed25519,
    /// `p256-pub` (multicodec `0x1200`), a 33-byte compressed point.
    P256,
    /// `p384-pub` (multicodec `0x1201`), a 49-byte compressed point.
    P384,
}

impl KeyType {
    /// The multicodec code for this key type.
    pub fn multicodec(self) -> u64 {
        match self {
            Self::Secp256k1 => 0xe7,
            Self::X25519 => 0xec,
            Self::Ed25519 => 0xed,
            Self::P256 => 0x1200,
            Self::P384 => 0x1201,
        }
    }

    /// The raw public key length, in bytes, that the did:key table
    /// requires for this key type.
    pub fn public_key_len(self) -> usize {
        match self {
            Self::X25519 | Self::Ed25519 => 32,
            Self::Secp256k1 | Self::P256 => 33,
            Self::P384 => 49,
        }
    }

    fn from_multicodec(code: u64) -> Option<Self> {
        [
            Self::Secp256k1,
            Self::X25519,
            Self::Ed25519,
            Self::P256,
            Self::P384,
        ]
        .into_iter()
        .find(|t| t.multicodec() == code)
    }
}

/// Error from [`DidKey::from_did`].
///
/// `#[non_exhaustive]`, so new variants stay semver-additive.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DidKeyError {
    /// The DID is not a `did:key`: the method is not `key`, there are
    /// not three or four colon-separated components, the version is not
    /// a positive integer, or the multibase value does not start with
    /// `z` (the did:key `invalidDid` error).
    InvalidDid,
    /// The multibase value has a character outside the base58-btc
    /// alphabet, is longer than any supported key needs, or does not
    /// start with a well-formed multicodec varint.
    InvalidMultibase,
    /// The multicodec is not a key type in the did:key table.
    UnsupportedKeyType {
        /// The decoded multicodec code.
        multicodec: u64,
    },
    /// The raw key has the wrong length for its key type (the did:key
    /// `invalidPublicKeyLength` error).
    InvalidPublicKeyLength {
        /// The key type the multicodec names.
        key_type: KeyType,
        /// The raw key length found, in bytes.
        len: usize,
    },
}

impl fmt::Display for DidKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDid => f.write_str("not a did:key with a base58-btc multibase value"),
            Self::InvalidMultibase => {
                f.write_str("did:key multibase value is not valid base58-btc multicodec")
            }
            Self::UnsupportedKeyType { multicodec } => {
                write!(
                    f,
                    "did:key multicodec {multicodec:#x} is not a supported key type"
                )
            }
            Self::InvalidPublicKeyLength { key_type, len } => write!(
                f,
                "did:key {key_type:?} public key is {len} bytes, expected {}",
                key_type.public_key_len()
            ),
        }
    }
}

impl std::error::Error for DidKeyError {}

/// A `did:key` decoded to its public key.
///
/// The key bytes are checked for length only. The did:key spec's
/// on-curve check ("It is not clear if this particular check is
/// implementable across all public key types") is left to the
/// cryptographic library that imports the key.
///
/// # Example
///
/// ```
/// use jmap_did_types::{Did, DidKey, KeyType};
///
/// let did: Did = "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK".parse()?;
/// let key = DidKey::from_did(&did)?;
/// assert_eq!(key.key_type(), KeyType::Ed25519);
/// assert_eq!(key.public_key().len(), 32);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DidKey {
    did: Did,
    key_type: KeyType,
    public_key: Vec<u8>,
}

impl DidKey {
    /// Decodes `did` per the did:key Decode Public Key Algorithm and
    /// checks the key length against the did:key table.
    ///
    /// # Errors
    ///
    /// Returns a [`DidKeyError`] when `did` is not a `did:key`, its
    /// multibase value is not base58-btc multicodec, the key type is not
    /// in the table, or the key has the wrong length.
    pub fn from_did(did: &Did) -> Result<Self, DidKeyError> {
        let multibase = multibase_value(did).ok_or(DidKeyError::InvalidDid)?;
        let bytes = decode_base58btc(multibase).ok_or(DidKeyError::InvalidMultibase)?;
        let (multicodec, raw) = decode_varint(&bytes).ok_or(DidKeyError::InvalidMultibase)?;
        let key_type = KeyType::from_multicodec(multicodec)
            .ok_or(DidKeyError::UnsupportedKeyType { multicodec })?;
        if raw.len() != key_type.public_key_len() {
            return Err(DidKeyError::InvalidPublicKeyLength {
                key_type,
                len: raw.len(),
            });
        }
        Ok(Self {
            did: did.clone(),
            key_type,
            public_key: raw.to_vec(),
        })
    }

    /// The DID this key was decoded from.
    pub fn did(&self) -> &Did {
        &self.did
    }

    /// The key type named by the multicodec prefix.
    pub fn key_type(&self) -> KeyType {
        self.key_type
    }

    /// The raw public key bytes that follow the multicodec prefix.
    pub fn public_key(&self) -> &[u8] {
        &self.public_key
    }

    /// The DID document, built by the did:key Document Creation
    /// Algorithm with `publicKeyFormat` `Multikey`, encryption key
    /// derivation off, and the `https://www.w3.org/ns/did/v1.1` context
    /// that the algorithm's context table gives `Multikey` (as in the
    /// spec's own Create example).
    ///
    /// The one verification method is listed in `authentication`,
    /// `assertionMethod`, `capabilityInvocation` and
    /// `capabilityDelegation`. No `keyAgreement` is derived.
    pub fn document(&self) -> Value {
        let id = self.did.as_str();
        let multibase = id.rsplit(':').next().unwrap_or(id);
        let vm_id = format!("{id}#{multibase}");
        json!({
            "@context": ["https://www.w3.org/ns/did/v1.1"],
            "id": id,
            "verificationMethod": [{
                "id": vm_id,
                "type": "Multikey",
                "controller": id,
                "publicKeyMultibase": multibase,
            }],
            "authentication": [vm_id],
            "assertionMethod": [vm_id],
            "capabilityInvocation": [vm_id],
            "capabilityDelegation": [vm_id],
        })
    }
}

/// The `multibaseValue` of a `did:key`: the last of three components,
/// or of four when the third is a positive-integer version.
fn multibase_value(did: &Did) -> Option<&str> {
    if did.method() != "key" {
        return None;
    }
    let value = match did.method_specific_id().split_once(':') {
        None => did.method_specific_id(),
        Some((version, value)) => {
            if value.contains(':') || version.parse::<u64>().ok()? == 0 {
                return None;
            }
            value
        }
    };
    value.starts_with('z').then_some(value)
}

/// Decodes a `z`-prefixed base58-btc multibase value.
pub(crate) fn decode_base58btc(multibase: &str) -> Option<Vec<u8>> {
    let digits = multibase.strip_prefix('z')?;
    if digits.is_empty() || digits.len() > MAX_MULTIBASE_LEN {
        return None;
    }
    let mut little_endian: Vec<u8> = Vec::new();
    for c in digits.bytes() {
        let mut carry = BASE58_ALPHABET.iter().position(|&a| a == c)? as u32;
        for byte in &mut little_endian {
            carry += u32::from(*byte) * 58;
            *byte = carry as u8;
            carry >>= 8;
        }
        while carry > 0 {
            little_endian.push(carry as u8);
            carry >>= 8;
        }
    }
    let zeros = digits.bytes().take_while(|&c| c == b'1').count();
    little_endian.extend(std::iter::repeat_n(0, zeros));
    little_endian.reverse();
    Some(little_endian)
}

/// Encodes `bytes` as base58-btc, without a multibase prefix.
pub(crate) fn encode_base58btc(bytes: &[u8]) -> String {
    let mut little_endian: Vec<u8> = Vec::new();
    for &byte in bytes {
        let mut carry = u32::from(byte);
        for digit in &mut little_endian {
            carry += u32::from(*digit) << 8;
            *digit = (carry % 58) as u8;
            carry /= 58;
        }
        while carry > 0 {
            little_endian.push((carry % 58) as u8);
            carry /= 58;
        }
    }
    let zeros = bytes.iter().take_while(|&&b| b == 0).count();
    std::iter::repeat_n('1', zeros)
        .chain(
            little_endian
                .iter()
                .rev()
                .map(|&d| char::from(BASE58_ALPHABET[usize::from(d)])),
        )
        .collect()
}

/// Splits a minimally encoded unsigned varint (multiformats
/// unsigned-varint) off the front of `bytes`.
fn decode_varint(bytes: &[u8]) -> Option<(u64, &[u8])> {
    let mut value = 0u64;
    for (i, &byte) in bytes.iter().enumerate().take(9) {
        value |= u64::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            if i > 0 && byte == 0 {
                return None;
            }
            return Some((value, &bytes[i + 1..]));
        }
    }
    None
}
