//! The (peer domain, nonce) replay store
//! (draft-atwood-jmap-chat-federation-00 §Request Signing, check 5).

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Mutex, PoisonError};
use std::time::SystemTime;

use jmap_peer_auth::PeerDomain;

/// Remembers accepted (peer domain, `nonce`) pairs.
///
/// Check 5: "The receiver has not accepted a request carrying the same
/// peer domain and `nonce` before. The receiver MUST remember each
/// accepted (peer domain, `nonce`) pair at least until 60 seconds after
/// that signature's `expires`."
///
/// Implement this over a store shared by every process that accepts
/// requests for the same server, so a nonce accepted by one is refused by
/// the others. [`MemoryReplayStore`] serves a single process.
pub trait ReplayStore: Send + Sync {
    /// Records the pair until `forget_after`, unless it is already
    /// recorded. Returns `true` when the pair was not recorded (accept
    /// the request) and `false` when it was (a replay).
    ///
    /// The check and the record must be one atomic step, or two
    /// concurrent requests with the same nonce could both be accepted.
    /// `now` is the caller's clock, for stores that expire entries
    /// themselves.
    fn insert(
        &self,
        peer_domain: &PeerDomain,
        nonce: &str,
        forget_after: SystemTime,
        now: SystemTime,
    ) -> impl Future<Output = Result<bool, ReplayStoreError>> + Send;

    /// The time from which this store holds every pair accepted, or
    /// `None` if it has never lost one.
    ///
    /// §Replay: "A receiving server whose nonce cache is lost, for
    /// example on restart, SHOULD reject requests whose `created`
    /// precedes the restart, since it can no longer detect their
    /// replay." [`authenticate`](crate::authenticate) rejects a request
    /// whose `created` is before this time. A store that survives
    /// restarts returns `None`, or the time it last lost its contents.
    fn remembers_since(&self) -> Option<SystemTime>;
}

/// A [`ReplayStore`] failure: the pair's state is unknown, so the request
/// must not be accepted.
#[derive(Debug, thiserror::Error)]
#[error("replay store failed")]
pub struct ReplayStoreError(#[source] pub Box<dyn std::error::Error + Send + Sync>);

/// The size below which [`MemoryReplayStore`] never sweeps.
const MIN_SWEEP: usize = 1024;

/// An in-process [`ReplayStore`].
///
/// An entry past its `forget_after` counts as absent. Expired entries are
/// swept when the store has doubled in size since the last sweep, so the
/// sweep costs O(1) amortized per insert and the store holds at most
/// about twice the pairs accepted in one retention window (at most 420
/// seconds: `created` up to 60 seconds ahead, `expires` up to 300 after
/// it, plus the 60-second margin).
#[derive(Debug)]
pub struct MemoryReplayStore {
    inner: Mutex<Inner>,
    since: SystemTime,
}

#[derive(Debug)]
struct Inner {
    seen: HashMap<(PeerDomain, String), SystemTime>,
    sweep_at: usize,
}

impl Default for MemoryReplayStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryReplayStore {
    /// An empty store that remembers pairs from now on: call it at
    /// startup, so requests created before the restart are rejected
    /// ([`ReplayStore::remembers_since`]).
    pub fn new() -> Self {
        Self::started_at(SystemTime::now())
    }

    /// An empty store that remembers pairs from `since`, for a caller
    /// with its own clock.
    pub fn started_at(since: SystemTime) -> Self {
        Self {
            inner: Mutex::new(Inner {
                seen: HashMap::new(),
                sweep_at: MIN_SWEEP,
            }),
            since,
        }
    }
}

impl ReplayStore for MemoryReplayStore {
    async fn insert(
        &self,
        peer_domain: &PeerDomain,
        nonce: &str,
        forget_after: SystemTime,
        now: SystemTime,
    ) -> Result<bool, ReplayStoreError> {
        // The map is consistent after every statement that touches it,
        // so a panic elsewhere while holding the lock cannot corrupt it.
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let key = (peer_domain.clone(), nonce.to_owned());
        if inner.seen.get(&key).is_some_and(|until| now <= *until) {
            return Ok(false);
        }
        inner.seen.insert(key, forget_after);
        if inner.seen.len() >= inner.sweep_at {
            inner.seen.retain(|_, until| now <= *until);
            inner.sweep_at = (inner.seen.len() * 2).max(MIN_SWEEP);
        }
        Ok(true)
    }

    fn remembers_since(&self) -> Option<SystemTime> {
        Some(self.since)
    }
}
