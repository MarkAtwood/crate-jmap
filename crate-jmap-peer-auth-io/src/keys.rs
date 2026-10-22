//! The peer key cache (draft-atwood-jmap-chat-federation-00 §Key
//! Publication and §DNSSEC Key Binding).

use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use jmap_peer_auth::{usable_keys, PeerDomain, PeerSigningKey, Rejection};
use serde_json::Value;

use crate::dns::DnssecLookup;
use crate::fetch::{freshness_lifetime, Fetch};
use crate::Error;

/// The longest a peer domain's keys are cached. §Key Publication: "for no
/// longer than one hour".
pub const MAX_CACHE_LIFETIME: Duration = Duration::from_secs(3600);

/// The shortest interval between two fetches of one peer domain's keys,
/// whatever caused them, and the shortest freshness lifetime of a fetched
/// key set. §Key Publication: "It SHOULD NOT refetch the keys of one peer
/// domain, or repeat that query, more often than once every 60 seconds"
/// and "For this rule the freshness lifetime of a fetched key set is at
/// least 60 seconds".
pub const REFETCH_INTERVAL: Duration = Duration::from_secs(60);

/// The number of peer domains a new [`PeerKeys`] tracks; see
/// [`PeerKeys::with_max_entries`].
pub const DEFAULT_MAX_ENTRIES: NonZeroUsize = match NonZeroUsize::new(10_000) {
    Some(n) => n,
    None => unreachable!(),
};

/// A peer domain's usable keys and the identity it advertises.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerKeySet {
    /// The usable keys (§Key Publication, after the §DNSSEC Key Binding
    /// precedence rule). Pass them to
    /// [`SignedRequest::verify`](jmap_peer_auth::SignedRequest::verify).
    pub keys: Vec<PeerSigningKey>,
    /// The `ownerUserId` of the Session object that supplied the keys: the
    /// peer identity of §Identity Binding item 1. Check it with
    /// [`bind_identity`](jmap_peer_auth::bind_identity). `None` when the
    /// Session object has no string `ownerUserId`.
    pub owner_user_id: Option<String>,
}

/// A clock for cache expiry and the refetch interval.
type Clock = Box<dyn Fn() -> Instant + Send + Sync>;

#[derive(Default)]
struct DomainState {
    /// When the last fetch started, successful or not.
    last_fetch: Option<Instant>,
    cached: Option<Cached>,
}

struct Cached {
    /// [`usable_keys`]' result: the keys, or [`Rejection::DnssecBogus`].
    usable: Result<Vec<PeerSigningKey>, Rejection>,
    owner_user_id: Option<String>,
    fresh_until: Instant,
}

impl Cached {
    fn find(&self, kid: &str) -> Result<PeerKeySet, Rejection> {
        let keys = self.usable.as_ref().map_err(Clone::clone)?;
        if keys.iter().any(|k| k.kid() == kid) {
            Ok(PeerKeySet {
                keys: keys.clone(),
                owner_user_id: self.owner_user_id.clone(),
            })
        } else {
            Err(Rejection::UnknownKey)
        }
    }
}

/// Fetches, filters and caches the usable signing keys of peer domains.
///
/// A fetch GETs `https://<peer-domain>/.well-known/jmap` and queries the
/// TXT records at `_jmap-peer.<peer-domain>` together, then applies
/// [`usable_keys`]. The result is cached for the response's HTTP
/// freshness lifetime ([`freshness_lifetime`]), at least
/// [`REFETCH_INTERVAL`] and at most [`MAX_CACHE_LIFETIME`], and at most
/// the DNS answer's TTL.
///
/// [`keys_for`](Self::keys_for) answers from the cache while the entry is
/// fresh and lists the `kid`. Otherwise it fetches again, "bypassing its
/// cache", unless the last fetch of that domain started less than
/// [`REFETCH_INTERVAL`] ago: then the request "is rejected". The limit
/// covers every fetch. A response fresh for less than 60 seconds
/// (`no-store`, or no `Cache-Control`) is still used until the interval
/// ends, so its known `kid`s keep verifying; §Key Publication: "until the
/// 60-second interval since the last fetch ends, the receiving server MAY
/// keep using the keys it last fetched even if their HTTP freshness
/// lifetime is shorter". A DNS TTL shorter than 60 seconds still ends the
/// entry early (§DNSSEC Key Binding).
///
/// One fetch per domain runs at a time; concurrent callers for the same
/// domain wait for it and then read the cache.
pub struct PeerKeys<F, D> {
    fetcher: F,
    dns: D,
    clock: Clock,
    max_entries: NonZeroUsize,
    domains: Mutex<HashMap<PeerDomain, Arc<tokio::sync::Mutex<DomainState>>>>,
}

impl<F: std::fmt::Debug, D: std::fmt::Debug> std::fmt::Debug for PeerKeys<F, D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PeerKeys")
            .field("fetcher", &self.fetcher)
            .field("dns", &self.dns)
            .field("max_entries", &self.max_entries)
            .finish_non_exhaustive()
    }
}

impl<F: Fetch, D: DnssecLookup> PeerKeys<F, D> {
    /// A cache that fetches Session objects through `fetcher` and TXT
    /// records through `dns`, tracking at most [`DEFAULT_MAX_ENTRIES`]
    /// peer domains.
    pub fn new(fetcher: F, dns: D) -> Self {
        Self {
            fetcher,
            dns,
            clock: Box::new(Instant::now),
            max_entries: DEFAULT_MAX_ENTRIES,
            domains: Mutex::new(HashMap::new()),
        }
    }

    /// Tracks at most `max_entries` peer domains. Adding a domain to a
    /// full cache drops the idle domains with no fresh keys and no
    /// fetch in the last [`REFETCH_INTERVAL`], then the idle domain
    /// fetched longest ago.
    pub fn with_max_entries(mut self, max_entries: NonZeroUsize) -> Self {
        self.max_entries = max_entries;
        self
    }

    /// Reads the time from `clock` instead of [`Instant::now`].
    pub fn with_clock(mut self, clock: impl Fn() -> Instant + Send + Sync + 'static) -> Self {
        self.clock = Box::new(clock);
        self
    }

    /// The usable keys of `peer_domain`, which must include `kid`.
    ///
    /// # Errors
    ///
    /// - [`Error::Rejected`] with [`Rejection::UnknownKey`] when no usable
    ///   key has this `kid` after any fetch the rules allow, or with
    ///   [`Rejection::DnssecBogus`] when the DNSSEC answer is bogus.
    /// - [`Error::Fetch`] or [`Error::InvalidSession`] when this call's
    ///   fetch failed. Errors are not cached, but the failed fetch counts
    ///   towards [`REFETCH_INTERVAL`].
    pub async fn keys_for(&self, peer_domain: &PeerDomain, kid: &str) -> Result<PeerKeySet, Error> {
        let slot = self.slot(peer_domain);
        let mut state = slot.lock().await;
        let now = (self.clock)();
        let fresh = state.cached.as_ref().filter(|c| now < c.fresh_until);
        if let Some(Ok(found)) = fresh.map(|c| c.find(kid)) {
            return Ok(found);
        }
        let expired = state.cached.is_some() && fresh.is_none();
        let recently = state
            .last_fetch
            .is_some_and(|t| now.duration_since(t) < REFETCH_INTERVAL);
        if recently {
            return Err(match fresh.map(|c| c.find(kid)) {
                Some(Err(rejection)) => rejection,
                _ => Rejection::UnknownKey,
            }
            .into());
        }
        state.last_fetch = Some(now);
        match self.fetch(peer_domain, now).await {
            Ok(cached) => {
                let found = cached.find(kid);
                state.cached = Some(cached);
                Ok(found?)
            }
            Err(e) => {
                if expired {
                    state.cached = None;
                }
                Err(e)
            }
        }
    }

    async fn fetch(&self, peer_domain: &PeerDomain, now: Instant) -> Result<Cached, Error> {
        let url = format!("https://{peer_domain}/.well-known/jmap");
        let name = format!("_jmap-peer.{peer_domain}.");
        let (response, dns) = tokio::join!(self.fetcher.get(&url), self.dns.txt(&name));
        let response = response?;
        let session: Value =
            serde_json::from_slice(&response.body).map_err(|_| Error::InvalidSession)?;
        let session = session.as_object().ok_or(Error::InvalidSession)?;
        let peer_signing_keys = session.get("peerSigningKeys").unwrap_or(&Value::Null);
        let mut lifetime = freshness_lifetime(&response.headers, SystemTime::now())
            .clamp(REFETCH_INTERVAL, MAX_CACHE_LIFETIME);
        if let Some(ttl) = dns.ttl {
            lifetime = lifetime.min(ttl);
        }
        Ok(Cached {
            usable: usable_keys(peer_signing_keys, &dns.answer),
            owner_user_id: session
                .get("ownerUserId")
                .and_then(Value::as_str)
                .map(str::to_owned),
            fresh_until: now + lifetime,
        })
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<PeerDomain, Arc<tokio::sync::Mutex<DomainState>>>> {
        // The map is consistent after every statement that touches it,
        // so a panic elsewhere while holding the lock cannot corrupt it.
        self.domains.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn slot(&self, peer_domain: &PeerDomain) -> Arc<tokio::sync::Mutex<DomainState>> {
        let mut domains = self.lock();
        if let Some(slot) = domains.get(peer_domain) {
            return Arc::clone(slot);
        }
        if domains.len() >= self.max_entries.get() {
            let now = (self.clock)();
            // A slot that is locked has a fetch or lookup in progress.
            domains.retain(|_, slot| match slot.try_lock() {
                Ok(state) => {
                    state.cached.as_ref().is_some_and(|c| now < c.fresh_until)
                        || state
                            .last_fetch
                            .is_some_and(|t| now.duration_since(t) < REFETCH_INTERVAL)
                }
                Err(_) => true,
            });
        }
        if domains.len() >= self.max_entries.get() {
            // ponytail: O(n) scan for the domain fetched longest ago,
            // only on a full cache with nothing to drop. Swap in an LRU
            // if that gets hot.
            let oldest = domains
                .iter()
                .filter_map(|(domain, slot)| {
                    slot.try_lock()
                        .ok()
                        .map(|state| (state.last_fetch, domain.clone()))
                })
                .min()
                .map(|(_, domain)| domain);
            if let Some(oldest) = oldest {
                domains.remove(&oldest);
            }
        }
        Arc::clone(domains.entry(peer_domain.clone()).or_default())
    }
}
