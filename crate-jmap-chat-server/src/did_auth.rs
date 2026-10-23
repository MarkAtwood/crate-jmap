//! DID-bound verification of a peer identity (draft-atwood-jmap-chat-did-00
//! §Federation Authentication).
//!
//! The draft keeps draft-atwood-jmap-chat-federation-00's request signing
//! as the only signature mechanism: "This document does not change that
//! mechanism and defines no other." A request is first verified against
//! the peer domain's `peerSigningKeys` (for example with
//! [`jmap_peer_auth::SignedRequest::verify`], whose
//! [`jmap_peer_auth::Accepted`] carries the key that verified it). Then,
//! for a DID-form peer identity, §DID-Bound Verification: "It takes the
//! Ed25519 public key that verified the request signature ... The
//! verification succeeds only if the `authentication` verification
//! relationship ({{DID-CORE}}) of the resolved DID Document contains a
//! verification method whose public key is an Ed25519 public key
//! ({{RFC8032}}) equal, octet for octet, to that key."
//! [`verify_did_binding`] is that check. It does not verify a signature
//! or record a nonce.
//!
//! "For a `did:web` peer identity, {{JMAP-CHAT-FED}} binds the identity by
//! the domain name it encodes and does not require this verification".
//!
//! This crate does no I/O. The caller resolves the DID and passes the
//! resolved DID Document as a [`DidDocument`], or `None` when resolution
//! failed and no usable cached document is held.
//!
//! §Caching: "If the key that verified a request signature is not in the
//! `authentication` verification relationship of the cached DID Document
//! ..., the server MUST resolve the DID again, bypassing its cache,
//! before failing the verification." A [`DidDocument::Cached`] document
//! whose `authentication` keys do not include the signing key yields
//! [`DidAuthRejection::RefreshRequired`]: the caller refreshes the
//! document (for example with `jmap_did_resolver::Resolver::refresh`,
//! which applies the draft's "once every 60 seconds" limit) and checks
//! once more with [`DidDocument::Refreshed`].

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jmap_did_types::{Did, DidKey, KeyType};
use jmap_peer_auth::ed25519_dalek::VerifyingKey;
use serde_json::Value;

/// A resolved DID Document and whether it may come from a cache.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DidDocument<'a> {
    /// A document that may be cached, such as one from
    /// `jmap_did_resolver::Resolver::resolve`.
    Cached(&'a Value),
    /// A document resolved again after a [`DidAuthRejection::RefreshRequired`],
    /// or one that is never cached, such as a document derived from a
    /// `did:key`.
    Refreshed(&'a Value),
}

/// Why a DID-form peer identity is not bound to the signing key.
///
/// draft-atwood-jmap-chat-did-00 §DID-Bound Verification: "If the
/// verification fails, including because the DID cannot be resolved
/// ({{resolution}}), the identity binding of {{JMAP-CHAT-FED}} does not
/// hold and the receiving server rejects the request."
///
/// `#[non_exhaustive]`, so new variants stay semver-additive.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DidAuthRejection {
    /// No usable DID Document: resolution failed and nothing usable is
    /// cached. §Resolution failure handling: "the DID-bound verification
    /// fails ({{did-verification}}) and the request is rejected."
    Unresolved,
    /// The document's `id` is not the DID. W3C DID Core §7.1: "The value
    /// of id in the resolved DID document MUST match the DID that was
    /// resolved."
    DocumentIdMismatch,
    /// No Ed25519 key of the document's `authentication` relationship
    /// equals the signing key.
    KeyNotBound,
    /// The signing key is absent from a [`DidDocument::Cached`]
    /// document's `authentication` keys. Refresh the document and check
    /// once more with [`DidDocument::Refreshed`] before rejecting.
    RefreshRequired,
}

impl fmt::Display for DidAuthRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unresolved => f.write_str("DID could not be resolved"),
            Self::DocumentIdMismatch => f.write_str("DID document id does not match the DID"),
            Self::KeyNotBound => {
                f.write_str("signing key is not in the DID document's authentication keys")
            }
            Self::RefreshRequired => {
                f.write_str("signing key is not in the cached DID document's authentication keys")
            }
        }
    }
}

impl std::error::Error for DidAuthRejection {}

/// Checks that `signing_key`, the `peerSigningKeys` key that verified the
/// request signature, is bound to `did` (draft-atwood-jmap-chat-did-00
/// §DID-Bound Verification steps 2 and 3).
///
/// Call it only after the request signature has verified: "After the
/// request signature has verified ({{JMAP-CHAT-FED}}), the receiving
/// server verifies a DID-form peer identity as follows". `document` is
/// the resolved DID Document of `did`, or `None` when resolution failed
/// and no usable cached document is held. Pass a document that may be
/// cached as [`DidDocument::Cached`].
///
/// The `authentication` entries are embedded verification methods or
/// references to `verificationMethod` entries, absolute or `#fragment`
/// relative to the DID (W3C DID Core §5.3.1, §3.2.2). A method is an
/// Ed25519 key when it has a `publicKeyJwk` with `kty` `OKP`, `crv`
/// `Ed25519` and no private member `d`, or a `publicKeyMultibase` that
/// decodes as an `ed25519-pub` multikey. Other methods are ignored.
///
/// # Errors
///
/// [`DidAuthRejection::Unresolved`] when `document` is `None`,
/// [`DidAuthRejection::DocumentIdMismatch`] when its `id` is not `did`,
/// and, when no `authentication` key equals `signing_key`,
/// [`DidAuthRejection::RefreshRequired`] for a [`DidDocument::Cached`]
/// document or [`DidAuthRejection::KeyNotBound`] for a
/// [`DidDocument::Refreshed`] one.
pub fn verify_did_binding(
    did: &Did,
    document: Option<DidDocument<'_>>,
    signing_key: &VerifyingKey,
) -> Result<(), DidAuthRejection> {
    let (document, cached) = match document.ok_or(DidAuthRejection::Unresolved)? {
        DidDocument::Cached(document) => (document, true),
        DidDocument::Refreshed(document) => (document, false),
    };
    if document.get("id").and_then(Value::as_str) != Some(did.as_str()) {
        return Err(DidAuthRejection::DocumentIdMismatch);
    }
    if authentication_keys(did, document).any(|key| &key == signing_key.as_bytes()) {
        Ok(())
    } else if cached {
        Err(DidAuthRejection::RefreshRequired)
    } else {
        Err(DidAuthRejection::KeyNotBound)
    }
}

/// The Ed25519 public keys of the document's `authentication` relationship.
fn authentication_keys<'a>(
    did: &'a Did,
    document: &'a Value,
) -> impl Iterator<Item = [u8; 32]> + 'a {
    // ponytail: only `#fragment` relative DID URLs are resolved; a
    // relative path or query reference (RFC 3986 §5) matches nothing.
    let absolute = move |id: &str| match id.strip_prefix('#') {
        Some(_) => format!("{did}{id}"),
        None => id.to_owned(),
    };
    let methods: Vec<&Value> = document
        .get("verificationMethod")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect();
    document
        .get("authentication")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(move |entry| match entry {
            Value::String(reference) => {
                let reference = absolute(reference);
                methods.iter().copied().find(|method| {
                    method
                        .get("id")
                        .and_then(Value::as_str)
                        .map(absolute)
                        .as_deref()
                        == Some(reference.as_str())
                })
            }
            Value::Object(_) => Some(entry),
            _ => None,
        })
        .filter_map(ed25519_key)
}

/// The 32-octet Ed25519 public key of a verification method, if it has one.
fn ed25519_key(method: &Value) -> Option<[u8; 32]> {
    if let Some(jwk) = method.get("publicKeyJwk") {
        let member = |name| jwk.get(name).and_then(Value::as_str);
        if member("kty")? != "OKP" || member("crv")? != "Ed25519" || jwk.get("d").is_some() {
            return None;
        }
        URL_SAFE_NO_PAD.decode(member("x")?).ok()?.try_into().ok()
    } else {
        // A did:key's method-specific id is exactly a multikey
        // `publicKeyMultibase` value, so the did:key decoder reads it.
        let multibase = method.get("publicKeyMultibase")?.as_str()?;
        let key = DidKey::from_did(&format!("did:key:{multibase}").parse().ok()?).ok()?;
        if key.key_type() != KeyType::Ed25519 {
            return None;
        }
        key.public_key().try_into().ok()
    }
}
