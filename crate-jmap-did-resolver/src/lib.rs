//! DID resolution I/O and caching for the `jmap-*` crates.
//!
//! This is a support crate (workspace AGENTS.md §Support crates): it does
//! network I/O, and no `*-types` or `*-server` crate depends on it. The
//! method rules live in [`jmap_did_types`]; this crate fetches the bytes
//! those pure helpers check, and caches the result.
//!
//! [`Resolver`] covers the three methods draft-atwood-jmap-chat-did-00
//! §Mandatory DID Methods requires, and the `did:webvh` it recommends
//! (L179 at jmap-chat-spec 6bcd9d8):
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
//!   (draft-atwood-jmap-chat-did-00 L175 at jmap-chat-spec 6bcd9d8:
//!   resolution "including the operation-log integrity check defined by
//!   that protocol").
//! - `did:webvh`: a GET of the DID log at [`webvh_log_url`], verified
//!   by [`verify_webvh_log`] (draft-atwood-jmap-did-00 L352 at 6bcd9d8
//!   resolves it "as defined in {{DID-WEBVH}}"). When the log needs
//!   witnessing, the `did-witness.json` beside it is fetched too and the
//!   log verified again with it.
//!
//! ## The `didDocumentUri` hint
//!
//! [`Resolver::resolve_with_hint`] takes the `didDocumentUri` of the
//! account capability, "a convenience hint, not a binding constraint"
//! (draft-atwood-jmap-did-00 L194 at jmap-chat-spec 29e9a41):
//!
//! - `did:webvh`: the log is fetched from the hint, from any origin, and
//!   verified as one from the method-derived URL would be ("a log fetched
//!   from this URI is resolved as defined in {{did-webvh}}"). "The
//!   witness proofs file (`did-witness.json`) always comes from the
//!   method-derived URL".
//! - `did:web`: the hint is used only when its origin (scheme, host and
//!   port) is the origin of [`Did::web_document_url`]; otherwise it is
//!   ignored. did:web has no check beyond TLS to the controller's domain,
//!   which is what protects it (L421: "the DID document is hosted at a
//!   well-known URL under the DID controller's domain"), so a document
//!   from another origin would let the JMAP server pick the document that
//!   checks its own binding.
//! - `did:key` and `did:plc`: the hint is ignored. A `did:key` document is
//!   derived from the DID, and a `did:plc` audit log comes from the PLC
//!   directory.
//!
//! When the hint is used and its fetch fails, the method-derived URL is
//! fetched instead ("When `didDocumentUri` is non-null but unreachable
//! (network error, HTTP 4xx/5xx), clients SHOULD fall back to standard
//! method-specific resolution"). A document or log from the hint that
//! fails its check is an error, not a fallback.
//!
//! ## Caching
//!
//! draft-atwood-jmap-chat-did-00 §Caching (L233 at jmap-chat-spec
//! 6bcd9d8) leaves the policy to the
//! deployment except for one rule: "If the key that verified a request
//! signature is not in the `authentication` verification relationship of
//! the cached DID Document ..., the server MUST resolve the DID again,
//! bypassing its cache, before failing the verification. A server SHOULD
//! NOT resolve one DID again for this reason more often than once every
//! 60 seconds; a request that arrives within that interval and whose
//! signing key is absent from the cached DID Document fails the
//! verification." When the signing key is absent from a document from
//! [`Resolver::resolve`], call [`Resolver::refresh`] and check once more
//! with the result. Within [`DEFAULT_REFRESH_INTERVAL`] of the DID's last
//! forced refresh, [`Resolver::refresh`] does not fetch and returns what
//! [`Resolver::resolve`] would (see
//! [`Resolver::with_refresh_interval`]).
//!
//! A forced refresh drops the cached entry before it fetches, so a
//! failed refresh leaves nothing cached: §Resolution failure handling
//! (L237), "Servers SHOULD NOT silently downgrade to an unauthenticated mode or
//! proceed with stale cached state when the cache entry has been
//! explicitly invalidated, including by the refresh requirement above."
//!
//! ## `did:webvh` versions
//!
//! A `did:webvh` resolves to the whole verified [`WebvhLog`]: take
//! [`WebvhLog::current`] for a request being received, and
//! [`Resolver::resolve_webvh_version`] for an object accepted earlier
//! under a recorded `versionId` (draft-atwood-jmap-did-00 L358).
//!
//! Beside the TTL cache, the resolver keeps the newest verified log of
//! each `did:webvh` DID. It is not dropped by expiry or [`Resolver::refresh`]
//! and serves version lookups, because a verified version never changes.
//! It is also the rollback check of DIF did:webvh v1.0 §Security
//! Considerations: "Resolvers **SHOULD** remember the latest `versionId`
//! previously observed for a DID and **SHOULD** warn or fail resolution
//! when presented with a truncated log." A fetched log that lacks that
//! `versionId` fails with [`ResolveError::WebvhRollback`]. A log with an
//! invalid entry is returned but not cached: "resolvers **SHOULD NOT**
//! cache a DID that fails verification."
//!
//! Network access goes through the [`Fetch`] trait. [`HttpFetcher`] is
//! the reqwest implementation; tests and deployments with their own HTTP
//! stack supply another.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::future::Future;
use std::num::NonZeroUsize;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use jmap_did_types::{
    verify_plc_audit_log, verify_webvh_log, webvh_log_url, Did, DidKey, DidKeyError, PlcError,
    PlcState, PlcStatus, WebvhError, WebvhLog, WebvhVersion,
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

/// The shortest time between two forced refreshes of one DID by a new
/// [`Resolver`]; see [`Resolver::with_refresh_interval`].
///
/// draft-atwood-jmap-chat-did-00 §Caching: "A server SHOULD NOT resolve
/// one DID again for this reason more often than once every 60 seconds".
pub const DEFAULT_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

/// The cache size of a new [`Resolver`], and the number of DIDs whose
/// last forced refresh it remembers; see [`Resolver::with_max_entries`].
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
    /// The verified DID log of a `did:webvh`. Its
    /// [`current`](WebvhLog::current) version is `None` when an entry is
    /// invalid or the DID is deactivated.
    Webvh(WebvhLog),
}

/// Error from [`Resolver::resolve`] or [`Resolver::refresh`].
///
/// draft-atwood-jmap-chat-did-00 §Resolution failure handling (L237 at
/// jmap-chat-spec 6bcd9d8): when resolution fails "and the server holds no
/// usable cached DID Document for that DID, the DID-bound verification
/// fails ({{did-verification}}) and the request is rejected." This
/// resolver never serves an expired or invalidated entry, so every one of
/// these is such a failure.
///
/// `#[non_exhaustive]`, so new variants stay semver-additive.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ResolveError {
    /// The DID method is not `key`, `web`, `plc` or `webvh`, or not
    /// `webvh` for [`Resolver::resolve_webvh_version`].
    #[error("DID method {0:?} is not supported")]
    UnsupportedMethod(String),
    /// A `did:web` with no usable domain (see
    /// [`Did::web_domain`]), a
    /// `did:plc` whose identifier is not 24 base32 characters, or a
    /// `did:webvh` with no [`webvh_log_url`].
    #[error("DID is not a valid did:web, did:plc or did:webvh identifier")]
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
    /// The `did:webvh` log failed verification.
    #[error("did:webvh log failed verification")]
    Webvh(#[source] WebvhError),
    /// The fetched `did:webvh` log does not contain the latest `versionId`
    /// this resolver verified before: it is truncated or forked.
    #[error("did:webvh log lacks a versionId seen before")]
    WebvhRollback,
    /// No verified version of the `did:webvh` has the requested
    /// `versionId`.
    #[error("did:webvh versionId not found")]
    VersionNotFound,
}

/// A clock for cache expiry.
type Clock = Box<dyn Fn() -> Instant + Send + Sync>;

struct Entry {
    resolved: Resolved,
    fetched_at: Instant,
}

/// Resolves `did:key`, `did:web`, `did:plc` and `did:webvh` DIDs and
/// caches fetched results for a fixed TTL.
///
/// An expired entry is fetched again on its next use; a failed fetch is
/// an error, never a stale result.
pub struct Resolver<F> {
    fetcher: F,
    ttl: Duration,
    refresh_interval: Duration,
    max_entries: NonZeroUsize,
    plc_directory: String,
    clock: Clock,
    cache: Mutex<HashMap<Did, Entry>>,
    /// When each DID was last force-refreshed. Kept apart from `cache` so
    /// that dropping an entry does not reset the limit.
    refreshed: Mutex<HashMap<Did, Instant>>,
    /// The newest verified log of each `did:webvh` DID and when it was
    /// stored: the rollback reference and the version store. Kept apart
    /// from `cache` so that expiry and refresh do not drop it.
    webvh: Mutex<HashMap<Did, (WebvhLog, Instant)>>,
}

impl<F: std::fmt::Debug> std::fmt::Debug for Resolver<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resolver")
            .field("fetcher", &self.fetcher)
            .field("ttl", &self.ttl)
            .field("refresh_interval", &self.refresh_interval)
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
            refresh_interval: DEFAULT_REFRESH_INTERVAL,
            max_entries: DEFAULT_MAX_ENTRIES,
            plc_directory: DEFAULT_PLC_DIRECTORY.to_owned(),
            clock: Box::new(Instant::now),
            cache: Mutex::new(HashMap::new()),
            refreshed: Mutex::new(HashMap::new()),
            webvh: Mutex::new(HashMap::new()),
        }
    }

    /// Uses the PLC directory at `base` (scheme and host, no trailing
    /// `/`) for `did:plc`.
    pub fn with_plc_directory(mut self, base: &str) -> Self {
        base.clone_into(&mut self.plc_directory);
        self
    }

    /// Caches at most `max_entries` DIDs, remembers the last forced
    /// refresh of at most `max_entries` DIDs, and keeps the newest log of
    /// at most `max_entries` `did:webvh` DIDs. Inserting into a full cache
    /// drops the expired entries, then the oldest one; the refresh record
    /// does the same, and the log store drops the oldest.
    pub fn with_max_entries(mut self, max_entries: NonZeroUsize) -> Self {
        self.max_entries = max_entries;
        self
    }

    /// Lets [`refresh`](Self::refresh) fetch one DID again at most once
    /// per `interval` instead of [`DEFAULT_REFRESH_INTERVAL`].
    /// [`Duration::ZERO`] lets every call fetch.
    pub fn with_refresh_interval(mut self, interval: Duration) -> Self {
        self.refresh_interval = interval;
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
        self.resolve_hinted(did, None).await
    }

    /// Resolves `did` as [`resolve`](Self::resolve) does, fetching from
    /// `did_document_uri`, the account capability's `didDocumentUri`, where
    /// the method allows it (see [the crate docs](crate#the-diddocumenturi-hint)).
    ///
    /// # Errors
    ///
    /// As [`resolve`](Self::resolve).
    pub async fn resolve_with_hint(
        &self,
        did: &Did,
        did_document_uri: &str,
    ) -> Result<Resolved, ResolveError> {
        self.resolve_hinted(did, Some(did_document_uri)).await
    }

    async fn resolve_hinted(
        &self,
        did: &Did,
        hint: Option<&str>,
    ) -> Result<Resolved, ResolveError> {
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
            None => self.fetch_and_store(did, hint).await,
        }
    }

    /// Resolves the version of a `did:webvh` DID whose `versionId` is
    /// `version_id`: the version in effect when an object was accepted
    /// (draft-atwood-jmap-did-00 L358 at jmap-chat-spec 6bcd9d8: "verify
    /// against that version").
    ///
    /// A version is looked up first in the newest log verified for `did`,
    /// which neither expiry nor [`refresh`](Self::refresh) drops, then in
    /// the result of [`resolve`](Self::resolve). A `versionId` newer than
    /// the cached log is found only after a [`refresh`](Self::refresh).
    ///
    /// # Errors
    ///
    /// [`ResolveError::UnsupportedMethod`] when `did` is not a
    /// `did:webvh`; [`ResolveError::VersionNotFound`] when no verified
    /// version has `version_id`; otherwise as [`resolve`](Self::resolve).
    pub async fn resolve_webvh_version(
        &self,
        did: &Did,
        version_id: &str,
    ) -> Result<WebvhVersion, ResolveError> {
        if did.method() != "webvh" {
            return Err(ResolveError::UnsupportedMethod(did.method().to_owned()));
        }
        let stored = relock(&self.webvh)
            .get(did)
            .and_then(|(log, _)| log.version(version_id).cloned());
        if let Some(version) = stored {
            return Ok(version);
        }
        match self.resolve(did).await? {
            Resolved::Webvh(log) => log.version(version_id).cloned(),
            _ => None,
        }
        .ok_or(ResolveError::VersionNotFound)
    }

    /// Drops the cached entry for `did` and resolves it again, unless
    /// `did` was last refreshed this way less than the refresh interval
    /// ago ([`with_refresh_interval`](Self::with_refresh_interval)): then
    /// it returns what [`resolve`](Self::resolve) returns, which is the
    /// cached entry while it is younger than the TTL.
    ///
    /// Call this when the key that verified a request signature is not in
    /// the `authentication` relationship of a document from
    /// [`resolve`](Self::resolve) (draft-atwood-jmap-chat-did-00
    /// §Caching), then check once more with the result. If the key is
    /// still absent, the verification fails.
    ///
    /// # Errors
    ///
    /// As [`resolve`](Self::resolve). After a failed forced refresh the
    /// entry stays dropped.
    pub async fn refresh(&self, did: &Did) -> Result<Resolved, ResolveError> {
        if did.method() == "key" {
            return derive_key(did);
        }
        if !self.start_refresh(did) {
            return self.resolve(did).await;
        }
        self.lock().remove(did);
        self.fetch_and_store(did, None).await
    }

    /// Records a forced refresh of `did` now, unless the last one was
    /// less than the refresh interval ago. Returns whether it recorded.
    fn start_refresh(&self, did: &Did) -> bool {
        let now = (self.clock)();
        let fresh = |at: &Instant| now.duration_since(*at) < self.refresh_interval;
        let mut refreshed = relock(&self.refreshed);
        if refreshed.get(did).is_some_and(fresh) {
            return false;
        }
        if refreshed.len() >= self.max_entries.get() && !refreshed.contains_key(did) {
            refreshed.retain(|_, at| fresh(at));
        }
        self.make_room(&mut refreshed, did, |at| *at);
        refreshed.insert(did.clone(), now);
        true
    }

    /// Drops the oldest value of a full `map` that lacks `did`.
    fn make_room<V>(&self, map: &mut HashMap<Did, V>, did: &Did, at: impl Fn(&V) -> Instant) {
        if map.len() >= self.max_entries.get() && !map.contains_key(did) {
            // ponytail: O(n) scan for the oldest entry, only on a full
            // map. Swap in an LRU if that gets hot.
            let oldest = map
                .iter()
                .min_by_key(|(_, value)| at(value))
                .map(|(did, _)| did.clone());
            if let Some(oldest) = oldest {
                map.remove(&oldest);
            }
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<Did, Entry>> {
        relock(&self.cache)
    }

    async fn fetch_and_store(
        &self,
        did: &Did,
        hint: Option<&str>,
    ) -> Result<Resolved, ResolveError> {
        let resolved = self.fetch(did, hint).await?;
        if matches!(&resolved, Resolved::Webvh(log) if log.invalid_entry().is_some()) {
            return Ok(resolved);
        }
        let now = (self.clock)();
        let mut cache = self.lock();
        if cache.len() >= self.max_entries.get() && !cache.contains_key(did) {
            cache.retain(|_, entry| now.duration_since(entry.fetched_at) < self.ttl);
        }
        self.make_room(&mut cache, did, |entry| entry.fetched_at);
        cache.insert(
            did.clone(),
            Entry {
                resolved: resolved.clone(),
                fetched_at: now,
            },
        );
        Ok(resolved)
    }

    async fn fetch(&self, did: &Did, hint: Option<&str>) -> Result<Resolved, ResolveError> {
        match did.method() {
            "web" => {
                let url = did.web_document_url().ok_or(ResolveError::InvalidDid)?;
                let hint = hint.filter(|hint| same_origin(hint, &url));
                let body = self.get_hinted(hint, &url).await?;
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
            "webvh" => {
                let url = webvh_log_url(did).ok_or(ResolveError::InvalidDid)?;
                let log = self.get_hinted(hint, &url).await?;
                let now = SystemTime::now();
                let mut verified = verify_webvh_log(did, &log, None, now);
                if lacks_witnesses(&verified) {
                    // "replacing the `/did.jsonl` at the end of the DID Log
                    // URL with `/did-witness.json`"
                    let dir = url.rsplit_once('/').map_or(url.as_str(), |(dir, _)| dir);
                    let witness = self.fetcher.get(&format!("{dir}/did-witness.json")).await?;
                    verified = verify_webvh_log(did, &log, Some(&witness), now);
                }
                let log = verified.map_err(ResolveError::Webvh)?;
                self.record_webvh(did, &log)?;
                Ok(Resolved::Webvh(log))
            }
            method => Err(ResolveError::UnsupportedMethod(method.to_owned())),
        }
    }

    /// GETs `hint` when there is one, and `url` when there is none or its
    /// fetch fails.
    async fn get_hinted(&self, hint: Option<&str>, url: &str) -> Result<Vec<u8>, FetchError> {
        if let Some(hint) = hint {
            if let Ok(body) = self.fetcher.get(hint).await {
                return Ok(body);
            }
        }
        self.fetcher.get(url).await
    }

    /// Fails when `log` lacks the latest `versionId` of the log stored for
    /// `did`, and otherwise stores `log` as the newest.
    fn record_webvh(&self, did: &Did, log: &WebvhLog) -> Result<(), ResolveError> {
        let mut stored = relock(&self.webvh);
        if let Some((seen, _)) = stored.get(did) {
            if log.version(&seen.latest().version_id).is_none() {
                return Err(ResolveError::WebvhRollback);
            }
        }
        self.make_room(&mut stored, did, |(_, at)| *at);
        stored.insert(did.clone(), (log.clone(), (self.clock)()));
        Ok(())
    }
}

/// Whether `hint` and `url` parse to the same origin (scheme, host and
/// port, the default port filled in).
fn same_origin(hint: &str, url: &str) -> bool {
    match (reqwest::Url::parse(hint), reqwest::Url::parse(url)) {
        (Ok(hint), Ok(url)) => hint.origin() == url.origin(),
        _ => false,
    }
}

/// Whether verifying without a witness proofs file found an entry that
/// needs one.
fn lacks_witnesses(verified: &Result<WebvhLog, WebvhError>) -> bool {
    let error = match verified {
        Ok(log) => log.invalid_entry().map(|(_, e)| e),
        Err(e) => Some(e),
    };
    error == Some(&WebvhError::WitnessThreshold)
}

fn relock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // Each map is consistent after every statement that touches it, so a
    // panic elsewhere while holding the lock cannot corrupt it.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
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
