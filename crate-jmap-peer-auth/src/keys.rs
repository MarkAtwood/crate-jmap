//! `peerSigningKeys` (draft-atwood-jmap-chat-federation-00 §Key
//! Publication) and the DNSSEC precedence rule (§DNSSEC Key Binding).

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::VerifyingKey;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::Rejection;

/// A usable `peerSigningKeys` entry: an Ed25519 public key and its `kid`,
/// the base64url SHA-256 JWK Thumbprint (RFC 7638) of the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerSigningKey {
    kid: String,
    key: VerifyingKey,
}

impl PeerSigningKey {
    /// Reads one `peerSigningKeys` entry, returning `None` for an entry
    /// that is not usable.
    ///
    /// §Key Publication: "A key is usable only if its `kty` and `crv` are
    /// as above and the receiving server, recomputing the thumbprint,
    /// obtains the key's `kid`. The receiving server MUST ignore any other
    /// entry." An entry is also ignored when `x` is not 32 base64url
    /// octets encoding an Ed25519 point, or when it carries the private
    /// member `d` (RFC 8037 §2), which a `peerSigningKeys` JWK "MUST NOT
    /// contain".
    pub fn from_jwk(jwk: &Value) -> Option<Self> {
        let jwk = jwk.as_object()?;
        let member = |name| jwk.get(name).and_then(Value::as_str);
        if member("kty")? != "OKP" || member("crv")? != "Ed25519" || jwk.contains_key("d") {
            return None;
        }
        let x: [u8; 32] = URL_SAFE_NO_PAD.decode(member("x")?).ok()?.try_into().ok()?;
        let key = Self::from(VerifyingKey::from_bytes(&x).ok()?);
        (member("kid")? == key.kid).then_some(key)
    }

    /// The JWK to publish in `peerSigningKeys`: `kty`, `crv`, `x` and
    /// `kid`, with no private members.
    pub fn to_jwk(&self) -> Value {
        json!({
            "kty": "OKP",
            "crv": "Ed25519",
            "x": URL_SAFE_NO_PAD.encode(self.key.as_bytes()),
            "kid": self.kid,
        })
    }

    /// The key's `kid` (its RFC 7638 thumbprint).
    pub fn kid(&self) -> &str {
        &self.kid
    }

    /// The Ed25519 public key.
    pub fn verifying_key(&self) -> &VerifyingKey {
        &self.key
    }
}

impl From<VerifyingKey> for PeerSigningKey {
    /// Computes the `kid` as the RFC 7638 thumbprint: SHA-256 over the
    /// required OKP members in lexicographic order (RFC 8037 §2,
    /// Appendix A.3), base64url-encoded.
    fn from(key: VerifyingKey) -> Self {
        let x = URL_SAFE_NO_PAD.encode(key.as_bytes());
        let canonical = format!(r#"{{"crv":"Ed25519","kty":"OKP","x":"{x}"}}"#);
        let kid = URL_SAFE_NO_PAD.encode(Sha256::digest(canonical));
        Self { kid, key }
    }
}

/// The DNSSEC validation result of the caller's `_jmap-peer.<peer-domain>`
/// TXT query (§DNSSEC Key Binding).
///
/// A caller without a DNSSEC-validating resolver, or without a trusted
/// channel to one, passes [`DnssecAnswer::Insecure`]: the draft says it
/// "MUST treat every zone as unsigned".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DnssecAnswer {
    /// A secure answer (RFC 4033 §5), with the value of each TXT record
    /// in it. A record whose character-strings are split is passed as
    /// their concatenation.
    Secure(Vec<String>),
    /// A secure denial of existence.
    SecureDenial,
    /// An insecure answer (the zone is unsigned).
    Insecure,
    /// The validation result is indeterminate (RFC 4033 §5).
    Indeterminate,
    /// The lookup failed, for example with SERVFAIL or a timeout.
    LookupFailed,
    /// The answer is bogus (RFC 4033 §5).
    Bogus,
}

/// The usable keys of a peer domain, from its `peerSigningKeys` value and
/// its DNSSEC answer.
///
/// Entries that [`PeerSigningKey::from_jwk`] rejects are ignored, as is a
/// `peerSigningKeys` value that is not an array. Under the DNSSEC
/// precedence rule, a [`DnssecAnswer::Secure`] answer with at least one
/// `v=jmappeer1; kid=<kid>` record keeps only the keys whose `kid` one of
/// those records lists; every other answer except
/// [`DnssecAnswer::Bogus`] uses the HTTPS-published keys alone.
///
/// # Errors
///
/// [`Rejection::DnssecBogus`] for a bogus answer: the receiving server
/// "MUST NOT fall back to the HTTPS-published keys and MUST reject the
/// request".
pub fn usable_keys(
    peer_signing_keys: &Value,
    dnssec: &DnssecAnswer,
) -> Result<Vec<PeerSigningKey>, Rejection> {
    let keys = peer_signing_keys
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(PeerSigningKey::from_jwk);
    match dnssec {
        DnssecAnswer::Bogus => Err(Rejection::DnssecBogus),
        DnssecAnswer::Secure(records) => {
            let kids: Vec<&str> = records
                .iter()
                .filter_map(|r| r.strip_prefix("v=jmappeer1; kid="))
                .filter(|kid| !kid.is_empty())
                .collect();
            if kids.is_empty() {
                Ok(keys.collect())
            } else {
                Ok(keys.filter(|k| kids.contains(&k.kid())).collect())
            }
        }
        DnssecAnswer::SecureDenial
        | DnssecAnswer::Insecure
        | DnssecAnswer::Indeterminate
        | DnssecAnswer::LookupFailed => Ok(keys.collect()),
    }
}
