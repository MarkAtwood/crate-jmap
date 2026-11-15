//! Digest algorithms (blobext-03 §2.1 `supportedDigestAlgorithms`, RFC 9530
//! §7.2 names) and the streaming [`Digest`] trait.

use std::cmp::Ordering;
use std::hash::{Hash, Hasher};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jmap_types::impl_string_enum;

/// Prefix of a digest property name: `digest:sha-256` (blobext-03 §4, §5).
const PREFIX: &str = "digest:";

/// A digest algorithm name from the IANA "Hash Algorithms for HTTP Digest
/// Fields" registry (RFC 9530 §7.2), as used in `digest:<algorithm>`
/// property names and `supportedDigestAlgorithms`.
///
/// Equality, ordering and hashing compare the wire string, so a forged
/// `Other("sha".into())` equals [`Sha`](Self::Sha), and a
/// `BTreeMap<DigestAlgorithm, _>` serializes its keys in wire-string order
/// (workspace "Wire-format maps" rule).
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum DigestAlgorithm {
    /// `sha-512`: SHA-512.
    Sha512,
    /// `sha-256`: SHA-256.
    Sha256,
    /// `md5`: MD5 (deprecated in the registry).
    Md5,
    /// `sha`: SHA-1 (deprecated in the registry; blobext-03 §2.1 lets a
    /// server still list it).
    Sha,
    /// `unixsum`: the UNIX `sum` command (deprecated).
    Unixsum,
    /// `unixcksum`: the UNIX `cksum` command (deprecated).
    Unixcksum,
    /// `adler`: ADLER32 (deprecated).
    Adler,
    /// `crc32c`: CRC32c (deprecated).
    Crc32c,
    /// A name the registry does not list.
    Other(String),
}

impl_string_enum!(DigestAlgorithm, "a digest algorithm name",
    "sha-512" => Sha512,
    "sha-256" => Sha256,
    "md5" => Md5,
    "sha" => Sha,
    "unixsum" => Unixsum,
    "unixcksum" => Unixcksum,
    "adler" => Adler,
    "crc32c" => Crc32c,
);

impl PartialEq for DigestAlgorithm {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for DigestAlgorithm {}

impl PartialOrd for DigestAlgorithm {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DigestAlgorithm {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl Hash for DigestAlgorithm {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl DigestAlgorithm {
    /// The property name for this algorithm: `digest:` followed by the
    /// algorithm name, e.g. `digest:sha-256`.
    pub fn property_name(&self) -> String {
        format!("{PREFIX}{}", self.as_str())
    }

    /// Parse a `digest:<algorithm>` property name.
    ///
    /// Returns `None` when `name` lacks the `digest:` prefix or the
    /// algorithm part is empty. Whether the algorithm is advertised is the
    /// caller's check.
    pub fn from_property_name(name: &str) -> Option<Self> {
        match name.strip_prefix(PREFIX) {
            Some(alg) if !alg.is_empty() => Some(Self::from(alg)),
            _ => None,
        }
    }

    /// A streaming digester for the algorithms this crate computes itself:
    /// `sha`, `sha-256` and `sha-512`. `None` for any other algorithm;
    /// a server that advertises one supplies its own digester.
    pub fn builtin_digester(&self) -> Option<Box<dyn Digest + Send>> {
        match self {
            Self::Sha => Some(Box::new(Builtin(<sha1::Sha1 as HashDigest>::new()))),
            Self::Sha256 => Some(Box::new(Builtin(<sha2::Sha256 as HashDigest>::new()))),
            Self::Sha512 => Some(Box::new(Builtin(<sha2::Sha512 as HashDigest>::new()))),
            // A forged `Other("sha")` names a built-in algorithm too.
            Self::Other(s) => match Self::from(s.as_str()) {
                Self::Other(_) => None,
                known => known.builtin_digester(),
            },
            _ => None,
        }
    }
}

/// A streaming digest computation.
///
/// Feed the octets with [`update`](Self::update) and take the value with
/// [`finish`](Self::finish): the digest in padded base64 (RFC 4648 §4), the
/// form blobext-03 `digest:*` properties carry.
pub trait Digest {
    /// Add `bytes` to the digest input.
    fn update(&mut self, bytes: &[u8]);
    /// Finish the computation and return the base64-encoded digest.
    fn finish(self: Box<Self>) -> String;
}

use sha2::digest::Digest as HashDigest;

/// A [`Digest`] over a RustCrypto hash.
struct Builtin<D>(D);

impl<D: HashDigest> Digest for Builtin<D> {
    fn update(&mut self, bytes: &[u8]) {
        HashDigest::update(&mut self.0, bytes);
    }

    fn finish(self: Box<Self>) -> String {
        STANDARD.encode(self.0.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_names() {
        assert_eq!(DigestAlgorithm::Sha256.property_name(), "digest:sha-256");
        assert_eq!(
            DigestAlgorithm::from_property_name("digest:sha"),
            Some(DigestAlgorithm::Sha)
        );
        assert_eq!(
            DigestAlgorithm::from_property_name("digest:foo"),
            Some(DigestAlgorithm::Other("foo".into()))
        );
        assert_eq!(DigestAlgorithm::from_property_name("digest:"), None);
        assert_eq!(DigestAlgorithm::from_property_name("sha"), None);
    }

    #[test]
    fn unsupported_algorithms_have_no_builtin_digester() {
        assert!(DigestAlgorithm::Md5.builtin_digester().is_none());
        assert!(
            DigestAlgorithm::Other("foo".into())
                .builtin_digester()
                .is_none()
        );
        assert!(
            DigestAlgorithm::Other("sha".into())
                .builtin_digester()
                .is_some()
        );
    }
}
