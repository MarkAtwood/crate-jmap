//! JMAP Chat federation peer authentication
//! (draft-atwood-jmap-chat-federation-00 §Peer Authentication Model).
//!
//! A peer is a domain. A server signs each request it sends to a peer
//! with HTTP Message Signatures (RFC 9421) under an Ed25519 key that its
//! peer domain publishes in `peerSigningKeys`; the receiving server
//! verifies the signature and so learns which peer domain sent the
//! request.
//!
//! ## Crate posture
//!
//! This crate holds the spec-defined parts that need no I/O:
//!
//! - No async dependencies, no network, DNS or persistence.
//! - The clock, the fetched keys and the DNSSEC validation result are
//!   passed in by the caller.
//! - Forbids `unsafe`.
//!
//! Fetching `peerSigningKeys` over HTTPS, the DNSSEC-validated
//! `_jmap-peer` TXT lookup, the key cache, the refetch rate limit and the
//! replay store belong to the caller (or a support crate).
//!
//! ## Public surface
//!
//! - [`sign`] — signs an outgoing request (§Request Signing) and returns
//!   the `Content-Digest`, `Signature-Input` and `Signature` header values.
//! - [`SignedRequest`] — parses a received request ([`SignedRequest::parse`],
//!   check 1 and the `keyid` part of check 2) and verifies it
//!   ([`SignedRequest::verify`], checks 2, 3, 4 and 6). Check 5 (replay)
//!   is the caller's atomic test-and-insert of the [`Accepted`] pair.
//!   Every [`Rejection`] maps to HTTP 403.
//! - [`PeerSigningKey`] — a usable `peerSigningKeys` entry (§Key
//!   Publication): Ed25519 OKP JWK whose `kid` is its RFC 7638 thumbprint.
//! - [`usable_keys`] and [`DnssecAnswer`] — the DNSSEC precedence rule
//!   (§DNSSEC Key Binding).
//! - [`bind_identity`] — the identity-host binding rule (§Identity
//!   Binding).
//! - [`PeerDomain`] — a peer domain in its compared form: lowercase
//!   A-label, no trailing dot, no port.
//! - [`fetch_address_allowed`] — the address rule of §Blob Fetch and SSRF
//!   for fetches from peer-supplied URLs.
//!
//! ## Example
//!
//! ```
//! use std::time::{Duration, UNIX_EPOCH};
//! use http::{HeaderMap, HeaderValue, Method};
//! use jmap_peer_auth::ed25519_dalek::SigningKey;
//! use jmap_peer_auth::{sign, PeerDomain, PeerSigningKey, Request, SignedRequest};
//!
//! let key = SigningKey::from_bytes(&[7; 32]);
//! let alice: PeerDomain = "alice.example".parse()?;
//! let now = UNIX_EPOCH + Duration::from_secs(1_760_000_000);
//! let body = br#"{"using":[]}"#;
//! let uri = "https://bob.example/jmap/api/";
//!
//! // Sender side.
//! let out = sign(&key, &alice, &Method::POST, uri, body, now)?;
//! let mut headers = HeaderMap::new();
//! headers.insert("content-digest", HeaderValue::from_str(out.content_digest.as_deref().unwrap())?);
//! headers.insert("signature-input", HeaderValue::from_str(&out.signature_input)?);
//! headers.insert("signature", HeaderValue::from_str(&out.signature)?);
//!
//! // Receiver side: the keys come from alice.example's peerSigningKeys.
//! let keys = [PeerSigningKey::from(key.verifying_key())];
//! let request = Request { method: &Method::POST, target_uri: uri, headers: &headers, content: body };
//! let signed = SignedRequest::parse(request)?;
//! let accepted = signed.verify(&keys, now)?;
//! assert_eq!(accepted.peer_domain, alice);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]

mod domain;
mod identity;
mod keys;
mod signature;
mod ssrf;

pub use domain::{InvalidPeerDomain, PeerDomain};
pub use ed25519_dalek;
pub use identity::{bind_identity, IdentityBinding};
pub use keys::{usable_keys, DnssecAnswer, PeerSigningKey};
pub use signature::{
    sign, Accepted, Rejection, Request, SignError, SignatureHeaders, SignedRequest,
    CLOCK_SKEW_SECS, MAX_LIFETIME_SECS, REPLAY_MARGIN_SECS, SIGNATURE_TAG,
};
pub use ssrf::fetch_address_allowed;
