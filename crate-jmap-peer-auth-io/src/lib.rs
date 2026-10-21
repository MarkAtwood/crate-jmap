//! JMAP Chat federation peer-authentication I/O
//! (draft-atwood-jmap-chat-federation-00 §Peer Authentication Model).
//!
//! This is a support crate (workspace AGENTS.md §Support crates): it does
//! network I/O, and no `*-types` or `*-server` crate depends on it. The
//! signature rules live in [`jmap_peer_auth`]; this crate supplies what
//! that crate takes as input.
//!
//! - [`PeerKeys`]: fetches a peer domain's `peerSigningKeys` with an
//!   unsigned GET of `https://<peer-domain>/.well-known/jmap` (§Key
//!   Publication), queries `_jmap-peer.<peer-domain>` (§DNSSEC Key
//!   Binding), applies [`usable_keys`](jmap_peer_auth::usable_keys), and
//!   caches the result for the HTTP freshness lifetime, at least 60
//!   seconds and at most one hour, and at most the DNS TTL. An unknown `kid` or an expired entry forces
//!   a refetch and a new DNS query, at most once every 60 seconds per peer
//!   domain, whatever the freshness lifetime.
//! - [`Fetch`] and [`HttpsFetcher`]: the HTTPS GET, with certificate
//!   validation against the host, `https`-only redirects and a
//!   caller-supplied address policy for §Blob Fetch and SSRF.
//! - [`DnssecLookup`], [`HickoryLookup`] (validates DNSSEC in-process)
//!   and [`NoDnssec`] (for a server that cannot validate).
//! - [`ReplayStore`] and [`MemoryReplayStore`]: verification check 5 and
//!   the §Replay restart rule.
//! - [`authenticate`]: all of the above around
//!   [`SignedRequest`].
//!
//! ## Example
//!
//! ```no_run
//! use std::time::SystemTime;
//! use jmap_peer_auth::Request;
//! use jmap_peer_auth_io::{authenticate, HickoryLookup, HttpsFetcher, MemoryReplayStore, PeerKeys};
//!
//! # async fn run(request: Request<'_>) -> Result<(), Box<dyn std::error::Error>> {
//! let keys = PeerKeys::new(HttpsFetcher::new()?, HickoryLookup::from_system_conf()?);
//! let replay = MemoryReplayStore::new();
//! let peer = authenticate(request, SystemTime::now(), &keys, &replay).await?;
//! println!("request from {} for {:?}", peer.peer_domain, peer.owner_user_id);
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]

mod dns;
mod fetch;
mod keys;
mod replay;

use std::time::SystemTime;

use jmap_peer_auth::{PeerDomain, PeerSigningKey, Rejection, Request, SignedRequest};

pub use dns::{DnsAnswer, DnssecLookup, HickoryLookup, NoDnssec};
pub use fetch::{
    freshness_lifetime, Fetch, FetchError, HttpsFetcher, Response, FETCH_TIMEOUT, MAX_BODY_BYTES,
    MAX_REDIRECTS,
};
pub use keys::{PeerKeySet, PeerKeys, DEFAULT_MAX_ENTRIES, MAX_CACHE_LIFETIME, REFETCH_INTERVAL};
pub use replay::{MemoryReplayStore, ReplayStore, ReplayStoreError};
pub use {hickory_resolver, jmap_peer_auth, reqwest};

/// Error from [`authenticate`] or [`PeerKeys::keys_for`].
///
/// §Request Signing: "A receiving server MUST reject, without processing
/// any method call, a request that lacks such a signature or whose
/// signature does not verify", with HTTP status 403. Every variant means
/// the request is not authenticated; [`Error::Fetch`],
/// [`Error::InvalidSession`] and [`Error::ReplayStore`] are failures to
/// find out, which a server may answer with a 5xx status instead.
///
/// `#[non_exhaustive]`, so new variants stay semver-additive.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A verification check failed.
    #[error("peer request rejected: {0}")]
    Rejected(#[from] Rejection),
    /// Fetching the peer domain's `/.well-known/jmap` failed.
    #[error("fetching the peer's keys failed")]
    Fetch(#[from] FetchError),
    /// The peer domain's `/.well-known/jmap` body is not a JSON object.
    #[error("the peer's /.well-known/jmap response is not a JSON object")]
    InvalidSession,
    /// The replay store failed.
    #[error("replay store failed")]
    ReplayStore(#[from] ReplayStoreError),
}

/// An authenticated peer request.
///
/// `#[non_exhaustive]`, so new fields stay semver-additive.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Authenticated {
    /// "The peer domain named in a verified `keyid` is the request's
    /// authenticated peer domain."
    pub peer_domain: PeerDomain,
    /// The peer domain's `peerSigningKeys` entry that verified the
    /// signature: draft-atwood-jmap-chat-did-00 §Federation Authentication
    /// step 2, "the key in the peer domain's `peerSigningKeys` whose `kid`
    /// the signature's `keyid` names".
    pub key: PeerSigningKey,
    /// The `ownerUserId` published at the peer domain, from the fetch
    /// that supplied the keys: the peer identity of §Identity Binding.
    /// Check it with [`bind_identity`](jmap_peer_auth::bind_identity)
    /// before granting peer-role access.
    pub owner_user_id: Option<String>,
}

/// Authenticates a received peer request (§Request Signing checks 1 to
/// 6).
///
/// Parses the `jmap-chat-peer` signature, finds the usable keys of its
/// peer domain through `keys`, verifies the signature at `now`, and
/// records the (peer domain, `nonce`) pair in `replay` until 60 seconds
/// after `expires`.
///
/// # Errors
///
/// An [`Error`] for the first check that fails; [`Rejection::Replayed`]
/// when `replay` already holds the pair; [`Rejection::PrecedesRestart`]
/// when `created` is before [`ReplayStore::remembers_since`].
pub async fn authenticate<F: Fetch, D: DnssecLookup, S: ReplayStore>(
    request: Request<'_>,
    now: SystemTime,
    keys: &PeerKeys<F, D>,
    replay: &S,
) -> Result<Authenticated, Error> {
    let signed = SignedRequest::parse(request)?;
    let set = keys.keys_for(signed.peer_domain(), signed.kid()).await?;
    // Check 5 is the atomic insert below rather than `verify`'s lookup:
    // a lookup followed by a separate record would let two concurrent
    // requests with the same nonce both pass.
    let accepted = signed.verify(&set.keys, now, |_, _| false)?;
    if replay
        .remembers_since()
        .is_some_and(|since| accepted.created < since)
    {
        return Err(Rejection::PrecedesRestart.into());
    }
    if !replay
        .insert(
            &accepted.peer_domain,
            &accepted.nonce,
            accepted.forget_after,
            now,
        )
        .await?
    {
        return Err(Rejection::Replayed.into());
    }
    Ok(Authenticated {
        peer_domain: accepted.peer_domain,
        key: accepted.key,
        owner_user_id: set.owner_user_id,
    })
}
