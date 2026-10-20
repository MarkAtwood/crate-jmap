//! DID resolution I/O and caching for the `jmap-*` crates.
//!
//! This is a support crate (workspace AGENTS.md §Support crates): it does
//! network I/O, and no `*-types` or `*-server` crate depends on it. The
//! method rules live in [`jmap_did_types`]; this crate fetches the bytes
//! those pure helpers check, and caches the result.
//!
//! [`Resolver`] covers the three methods draft-atwood-jmap-chat-did-00
//! §Mandatory DID Methods requires:
//!
//! - `did:key`: the document is derived from the DID
//!   ([`DidKey::document`]). No fetch,
//!   no cache.
//! - `did:web`: an HTTPS GET of
//!   [`Did::web_document_url`],
//!   then the did:web Read step "Verify that the ID of the resolved DID
//!   document matches the Web DID being resolved."
//! - `did:plc`: a GET of the PLC directory's `/<did>/log/audit`, checked
//!   by [`verify_plc_audit_log`]
//!   (draft-atwood-jmap-chat-did-00 §did:plc operation log integrity:
//!   "Implementations MUST verify operation-log integrity").
//!
//! ## Caching
//!
//! draft-atwood-jmap-chat-did-00 §Caching leaves the policy to the
//! deployment except for one rule: "A server MUST refresh its cached DID
//! Document for a given DID before next use if a signature verification
//! against that cached document fails." When a signature fails against a
//! document from [`Resolver::resolve`], call [`Resolver::refresh`] and
//! retry once with the result.
//!
//! [`Resolver::refresh`] drops the cached entry before it fetches, so a
//! failed refresh leaves nothing cached: §Resolution failure handling,
//! "Servers MUST NOT ... proceed with stale cached state when the cache
//! entry has been explicitly invalidated."
//!
//! Network access goes through the [`Fetch`] trait. [`HttpFetcher`] is
//! the reqwest implementation; tests and deployments with their own HTTP
//! stack supply another.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::future::Future;
use std::num::NonZeroUsize;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use jmap_did_types::{
    verify_plc_audit_log, Did, DidKey, DidKeyError, PlcError, PlcState, PlcStatus,
};
use serde_json::Value;

pub use jmap_did_types;
pub use reqwest;

/// The PLC directory [`Resolver::new`] uses: the did:plc Method
/// Specification v0.3.0 §DID Resolution endpoint host.
pub const DEFAULT_PLC_DIRECTORY: &str = "https://plc.directory";

/// The largest response body [`HttpFetcher`] accepts, in bytes.
///
/// ponytail: one fixed cap for every method. A did:web document or PLC
/// audit log is a few kilobytes; if a real log outgrows this, make it a
/// per-fetcher setting.
pub const MAX_BODY_BYTES: usize = 1 << 20;

/// The request timeout of [`HttpFetcher::new`].
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// The cache size of a new [`Resolver`]; see
/// [`Resolver::with_max_entries`].
pub const DEFAULT_MAX_ENTRIES: NonZeroUsize = match NonZeroUsize::new(10_000) {
    Some(n) => n,
    None => unreachable!(),
};

/// An HTTP GET.
///
/// Implementations return the body of a 2xx response, and
/// [`FetchError::Status`] for any other status.
pub trait Fetch: Send + Sync {
    /// GETs `url` and returns the response body.
    fn get(&self, url: &str) -> impl Future<Output = Result<Vec<u8>, FetchError>> + Send;
}

/// Error from [`Fetch::get`].
///
/// `#[non_exhaustive]`, so new variants stay semver-additive.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FetchError {
    /// The response status was not 2xx.
    #[error("HTTP status {0}")]
    Status(u16),
    /// The response body is longer than [`MAX_BODY_BYTES`].
    #[error("response body exceeds {MAX_BODY_BYTES} bytes")]
    TooLarge,
    /// The request failed before a complete response arrived: DNS, TLS,
    /// connection, timeout, a non-HTTPS URL on an HTTPS-only client.
    #[error("HTTP request failed")]
    Transport(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// A [`Fetch`] over a [`reqwest::Client`].
#[derive(Debug, Clone)]
pub struct HttpFetcher {
    client: reqwest::Client,
}

impl HttpFetcher {
    /// A fetcher that only makes HTTPS requests (redirects included) and
    /// gives up after [`FETCH_TIMEOUT`].
    ///
    /// did:web Read (Resolve): "Perform an HTTP GET request to the URL
    /// using an agent that can successfully negotiate a secure HTTPS
    /// connection".
    ///
    /// # Errors
    ///
    /// [`FetchError::Transport`] when reqwest cannot build the client
    /// (its TLS backend fails to initialize).
    pub fn new() -> Result<Self, FetchError> {
        reqwest::Client::builder()
            .https_only(true)
            .timeout(FETCH_TIMEOUT)
            .build()
            .map(Self::from_client)
            .map_err(|e| FetchError::Transport(Box::new(e)))
    }

    /// A fetcher over `client`, as configured. HTTPS-only, timeouts,
    /// proxies and TLS pinning are up to the caller.
    pub fn from_client(client: reqwest::Client) -> Self {
        Self { client }
    }
}

impl Fetch for HttpFetcher {
    async fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        let transport = |e: reqwest::Error| FetchError::Transport(Box::new(e));
        let mut response = self.client.get(url).send().await.map_err(transport)?;
        let status = response.status();
        if !status.is_success() {
            return Err(FetchError::Status(status.as_u16()));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(transport)? {
            if body.len() + chunk.len() > MAX_BODY_BYTES {
                return Err(FetchError::TooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }
}

/// The result of resolving a DID.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Resolved {
    /// A DID document: derived from a `did:key`, or fetched for a
    /// `did:web` and checked to have the DID as its `id`.
    Document(Value),
    /// The verified current state of an active `did:plc`.
    Plc(PlcState),
}

/// Error from [`Resolver::resolve`] or [`Resolver::refresh`].
///
/// draft-atwood-jmap-chat-did-00 §Resolution failure handling: on any of
/// these "the server MUST treat the DID as unverified and MUST reject any
/// authentication attempt that depends on it."
///
/// `#[non_exhaustive]`, so new variants stay semver-additive.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ResolveError {
    /// The DID method is not `key`, `web` or `plc`.
    #[error("DID method {0:?} is not supported")]
    UnsupportedMethod(String),
    /// A `did:web` with no usable domain (see
    /// [`Did::web_domain`]), or a
    /// `did:plc` whose identifier is not 24 base32 characters.
    #[error("DID is not a valid did:web or did:plc identifier")]
    InvalidDid,
    /// A `did:key` that does not decode.
    #[error("did:key does not decode")]
    Key(#[source] DidKeyError),
    /// The fetch failed.
    #[error("DID resolution fetch failed")]
    Fetch(#[from] FetchError),
    /// The fetched `did:web` document is not JSON.
    #[error("DID document is not JSON")]
    Json(#[source] serde_json::Error),
    /// The fetched `did:web` document's `id` is not the DID.
    #[error("DID document id does not match the DID")]
    IdMismatch,
    /// The `did:plc` audit log failed verification.
    #[error("did:plc audit log failed verification")]
    Plc(#[source] PlcError),
    /// The `did:plc` is not active: its latest operation is a tombstone.
    #[error("did:plc is deactivated")]
    Deactivated,
}

/// A clock for cache expiry.
type Clock = Box<dyn Fn() -> Instant + Send + Sync>;

struct Entry {
    resolved: Resolved,
    fetched_at: Instant,
}

/// Resolves `did:key`, `did:web` and `did:plc` DIDs and caches fetched
/// results for a fixed TTL.
///
/// An expired entry is fetched again on its next use; a failed fetch is
/// an error, never a stale result.
pub struct Resolver<F> {
    fetcher: F,
    ttl: Duration,
    max_entries: NonZeroUsize,
    plc_directory: String,
    clock: Clock,
    cache: Mutex<HashMap<Did, Entry>>,
}

impl<F: std::fmt::Debug> std::fmt::Debug for Resolver<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resolver")
            .field("fetcher", &self.fetcher)
            .field("ttl", &self.ttl)
            .field("max_entries", &self.max_entries)
            .field("plc_directory", &self.plc_directory)
            .finish_non_exhaustive()
    }
}

impl<F: Fetch> Resolver<F> {
    /// A resolver that fetches through `fetcher` and keeps fetched
    /// results for `ttl`, using [`DEFAULT_PLC_DIRECTORY`] and at most
    /// [`DEFAULT_MAX_ENTRIES`] entries.
    pub fn new(fetcher: F, ttl: Duration) -> Self {
        Self {
            fetcher,
            ttl,
            max_entries: DEFAULT_MAX_ENTRIES,
            plc_directory: DEFAULT_PLC_DIRECTORY.to_owned(),
            clock: Box::new(Instant::now),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Uses the PLC directory at `base` (scheme and host, no trailing
    /// `/`) for `did:plc`.
    pub fn with_plc_directory(mut self, base: &str) -> Self {
        base.clone_into(&mut self.plc_directory);
        self
    }

    /// Caches at most `max_entries` DIDs. Inserting into a full cache
    /// drops the expired entries, then the oldest one.
    pub fn with_max_entries(mut self, max_entries: NonZeroUsize) -> Self {
        self.max_entries = max_entries;
        self
    }

    /// Reads the time from `clock` instead of [`Instant::now`].
    pub fn with_clock(mut self, clock: impl Fn() -> Instant + Send + Sync + 'static) -> Self {
        self.clock = Box::new(clock);
        self
    }

    /// Resolves `did`, from the cache when its entry is younger than the
    /// TTL.
    ///
    /// # Errors
    ///
    /// A [`ResolveError`] when the DID cannot be resolved and verified.
    /// Errors are not cached.
    pub async fn resolve(&self, did: &Did) -> Result<Resolved, ResolveError> {
        if did.method() == "key" {
            return derive_key(did);
        }
        let now = (self.clock)();
        let cached = self
            .lock()
            .get(did)
            .filter(|entry| now.duration_since(entry.fetched_at) < self.ttl)
            .map(|entry| entry.resolved.clone());
        match cached {
            Some(resolved) => Ok(resolved),
            None => self.fetch_and_store(did).await,
        }
    }

    /// Drops the cached entry for `did` and resolves it again.
    ///
    /// Call this when a signature fails to verify against a document
    /// from [`resolve`](Self::resolve) (draft-atwood-jmap-chat-did-00
    /// §Caching), then retry the verification once with the result.
    ///
    /// # Errors
    ///
    /// As [`resolve`](Self::resolve). The entry stays dropped.
    pub async fn refresh(&self, did: &Did) -> Result<Resolved, ResolveError> {
        if did.method() == "key" {
            return derive_key(did);
        }
        self.lock().remove(did);
        self.fetch_and_store(did).await
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<Did, Entry>> {
        // The map is consistent after every statement that touches it,
        // so a panic elsewhere while holding the lock cannot corrupt it.
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
    }

    async fn fetch_and_store(&self, did: &Did) -> Result<Resolved, ResolveError> {
        let resolved = self.fetch(did).await?;
        let now = (self.clock)();
        let mut cache = self.lock();
        if cache.len() >= self.max_entries.get() && !cache.contains_key(did) {
            cache.retain(|_, entry| now.duration_since(entry.fetched_at) < self.ttl);
        }
        if cache.len() >= self.max_entries.get() && !cache.contains_key(did) {
            // ponytail: O(n) scan for the oldest entry, only on a full
            // cache with nothing expired. Swap in an LRU if that gets hot.
            let oldest = cache
                .iter()
                .min_by_key(|(_, entry)| entry.fetched_at)
                .map(|(did, _)| did.clone());
            if let Some(oldest) = oldest {
                cache.remove(&oldest);
            }
        }
        cache.insert(
            did.clone(),
            Entry {
                resolved: resolved.clone(),
                fetched_at: now,
            },
        );
        Ok(resolved)
    }

    async fn fetch(&self, did: &Did) -> Result<Resolved, ResolveError> {
        match did.method() {
            "web" => {
                let url = did.web_document_url().ok_or(ResolveError::InvalidDid)?;
                let body = self.fetcher.get(&url).await?;
                let document: Value = serde_json::from_slice(&body).map_err(ResolveError::Json)?;
                if document.get("id").and_then(Value::as_str) != Some(did.as_str()) {
                    return Err(ResolveError::IdMismatch);
                }
                Ok(Resolved::Document(document))
            }
            "plc" => {
                if !is_plc_identifier(did.method_specific_id()) {
                    return Err(ResolveError::InvalidDid);
                }
                let url = format!("{}/{did}/log/audit", self.plc_directory);
                let body = self.fetcher.get(&url).await?;
                match verify_plc_audit_log(did, &body).map_err(ResolveError::Plc)? {
                    PlcStatus::Active(state) => Ok(Resolved::Plc(state)),
                    _ => Err(ResolveError::Deactivated),
                }
            }
            method => Err(ResolveError::UnsupportedMethod(method.to_owned())),
        }
    }
}

fn derive_key(did: &Did) -> Result<Resolved, ResolveError> {
    DidKey::from_did(did)
        .map(|key| Resolved::Document(key.document()))
        .map_err(ResolveError::Key)
}

/// did:plc §Identifier Syntax: "The identifier part is 24 characters
/// long, including only characters from the `base32` encoding set"
/// (lowercase `a-z` and `2-7`).
fn is_plc_identifier(id: &str) -> bool {
    id.len() == 24 && id.bytes().all(|b| matches!(b, b'a'..=b'z' | b'2'..=b'7'))
}
