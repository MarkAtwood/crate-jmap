//! DID-keyed peer authentication (draft-atwood-jmap-chat-did-00
//! §Federation Authentication).
//!
//! A federated peer whose principal identity is a DID proves control of
//! that DID with "a cryptographic signature made by a public key bound in
//! the DID's DID Document". This module follows the draft's recommended
//! realization: "RFC 9421 HTTP Message Signatures using an Ed25519
//! signing key selected from the DID Document's `authentication`
//! verification relationship".
//!
//! The signature is the request's one `jmap-chat-peer` signature of
//! draft-atwood-jmap-chat-federation-00 §Request Signing, parsed by
//! [`SignedRequest::parse`]. [`verify_did_signature`] runs the same
//! checks against the DID's `authentication` keys instead of the peer
//! domain's `peerSigningKeys`: the `kid` in the `keyid` must be the
//! RFC 7638 thumbprint of one of those keys.
//!
//! This crate does no I/O. The caller resolves the DID and passes the
//! resolved DID Document as a [`DidDocument`], or `None` when resolution
//! failed and no usable cached document is held.
//!
//! §Caching: "A server MUST refresh its cached DID Document for a given
//! DID before next use if a signature verification against that cached
//! document fails." A [`DidDocument::Cached`] document whose keys do not
//! verify the signature yields [`DidAuthRejection::RefreshRequired`]:
//! the caller refreshes the document (for example with
//! `jmap_did_resolver::Resolver::refresh`) and verifies once more with
//! [`DidDocument::Refreshed`].

use std::fmt;
use std::time::SystemTime;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use jmap_did_types::{Did, DidKey, KeyType};
use jmap_peer_auth::ed25519_dalek::VerifyingKey;
use jmap_peer_auth::{Accepted, PeerDomain, PeerSigningKey, Rejection, SignedRequest};
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

/// Why a request is not authenticated for a DID.
///
/// draft-atwood-jmap-chat-did-00 §Signature requirement step 4: "If
/// verification fails, the receiving server SHOULD reject the request
/// with an authentication error."
///
/// `#[non_exhaustive]`, so new variants stay semver-additive.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DidAuthRejection {
    /// No usable DID Document: resolution failed and nothing usable is
    /// cached. §Resolution failure handling: "the server SHOULD treat the
    /// DID as unverified and SHOULD reject any authentication attempt
    /// that depends on it."
    Unresolved,
    /// The document's `id` is not the DID. W3C DID Core §7.1: "The value
    /// of id in the resolved DID document MUST match the DID that was
    /// resolved."
    DocumentIdMismatch,
    /// The signature failed a check against the DID's `authentication`
    /// keys. [`Rejection::UnknownKey`] means the `kid` names none of them.
    Signature(Rejection),
    /// A [`DidDocument::Cached`] document's keys did not verify the
    /// signature: the `kid` names none of them
    /// ([`Rejection::UnknownKey`]) or the signature does not verify under
    /// the key ([`Rejection::BadSignature`]). Refresh the document and
    /// verify once more with [`DidDocument::Refreshed`] before rejecting.
    RefreshRequired(Rejection),
}

impl fmt::Display for DidAuthRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unresolved => f.write_str("DID could not be resolved"),
            Self::DocumentIdMismatch => f.write_str("DID document id does not match the DID"),
            Self::Signature(_) => f.write_str("DID-bound signature did not verify"),
            Self::RefreshRequired(_) => {
                f.write_str("DID-bound signature did not verify against a cached DID document")
            }
        }
    }
}

impl std::error::Error for DidAuthRejection {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Signature(rejection) | Self::RefreshRequired(rejection) => Some(rejection),
            Self::Unresolved | Self::DocumentIdMismatch => None,
        }
    }
}

/// Verifies that `signed` was made by a key in `did`'s DID Document
/// `authentication` relationship (draft-atwood-jmap-chat-did-00
/// §Signature requirement steps 2 to 5).
///
/// `document` is the resolved DID Document of `did`, or `None` when
/// resolution failed and no usable cached document is held. Pass a
/// document that may be cached as [`DidDocument::Cached`]. `now` and
/// `seen` are as for [`SignedRequest::verify`]. A caller that also
/// verifies `signed` against the peer domain's keys calls `seen` from
/// both before it records the nonce.
///
/// The `authentication` entries are embedded verification methods or
/// references to `verificationMethod` entries, absolute or `#fragment`
/// relative to the DID (W3C DID Core §5.3.1, §3.2.2). A method is used
/// when it is an Ed25519 key: a `publicKeyJwk` with `kty` `OKP`, `crv`
/// `Ed25519` and no private member `d`, or a `publicKeyMultibase` that
/// decodes as an `ed25519-pub` multikey. Other methods are ignored.
///
/// On success the request is "authenticated for the claimed DID"
/// (step 5); record the returned nonce as [`SignedRequest::verify`]
/// describes.
///
/// # Errors
///
/// [`DidAuthRejection::Unresolved`] when `document` is `None`,
/// [`DidAuthRejection::DocumentIdMismatch`] when its `id` is not `did`,
/// and [`DidAuthRejection::Signature`] for the first signature check that
/// fails, except that a [`DidDocument::Cached`] document failing
/// [`Rejection::UnknownKey`] or [`Rejection::BadSignature`] gives
/// [`DidAuthRejection::RefreshRequired`]. The other checks do not depend
/// on the document, so a refresh cannot change their outcome.
pub fn verify_did_signature(
    signed: &SignedRequest<'_>,
    did: &Did,
    document: Option<DidDocument<'_>>,
    now: SystemTime,
    seen: impl FnOnce(&PeerDomain, &str) -> bool,
) -> Result<Accepted, DidAuthRejection> {
    let (document, cached) = match document.ok_or(DidAuthRejection::Unresolved)? {
        DidDocument::Cached(document) => (document, true),
        DidDocument::Refreshed(document) => (document, false),
    };
    if document.get("id").and_then(Value::as_str) != Some(did.as_str()) {
        return Err(DidAuthRejection::DocumentIdMismatch);
    }
    let keys = authentication_keys(did, document);
    signed
        .verify(&keys, now, seen)
        .map_err(|rejection| match rejection {
            Rejection::UnknownKey | Rejection::BadSignature if cached => {
                DidAuthRejection::RefreshRequired(rejection)
            }
            rejection => DidAuthRejection::Signature(rejection),
        })
}

/// The Ed25519 keys of the document's `authentication` relationship.
fn authentication_keys(did: &Did, document: &Value) -> Vec<PeerSigningKey> {
    // ponytail: only `#fragment` relative DID URLs are resolved; a
    // relative path or query reference (RFC 3986 §5) matches nothing.
    let absolute = |id: &str| match id.strip_prefix('#') {
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
        .filter_map(|entry| match entry {
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
        .collect()
}

/// The Ed25519 key of a verification method, if it has one.
fn ed25519_key(method: &Value) -> Option<PeerSigningKey> {
    let bytes: [u8; 32] = if let Some(jwk) = method.get("publicKeyJwk") {
        let member = |name| jwk.get(name).and_then(Value::as_str);
        if member("kty")? != "OKP" || member("crv")? != "Ed25519" || jwk.get("d").is_some() {
            return None;
        }
        URL_SAFE_NO_PAD.decode(member("x")?).ok()?.try_into().ok()?
    } else {
        // A did:key's method-specific id is exactly a multikey
        // `publicKeyMultibase` value, so the did:key decoder reads it.
        let multibase = method.get("publicKeyMultibase")?.as_str()?;
        let key = DidKey::from_did(&format!("did:key:{multibase}").parse().ok()?).ok()?;
        if key.key_type() != KeyType::Ed25519 {
            return None;
        }
        key.public_key().try_into().ok()?
    };
    Some(PeerSigningKey::from(VerifyingKey::from_bytes(&bytes).ok()?))
}
