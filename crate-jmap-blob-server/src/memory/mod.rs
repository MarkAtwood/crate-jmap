//! In-memory reference [`BlobBackend`] (docs/design/blobext.md §7).
//!
//! Not production: a reference implementation, an integration-test harness
//! and a documentation example.
//!
//! - Blobs live per account; an account must be registered first
//!   ([`MemoryBackend::with_account`], [`MemoryBackend::register_account`]).
//!   Ids are `B` plus a counter.
//! - [`create_blob`](BlobBackend::create_blob) stores a blob made only of
//!   inline data as bytes. A blob with any blobId source is stored as a
//!   chunk list: blobId sources become chunks of their (leaf) blobs, and
//!   inline pieces become new blobs of their own, so `Blob/get` `chunks`
//!   reports real blobIds (blobext-03 §5, §9.2). A blob used as a chunk by
//!   another blob cannot be destroyed (`blobHasReference`).
//! - `expires`: one hour after creation, five minutes for `noPersist`, and
//!   `null` while an object references the blob. A touch is honoured up to
//!   seven days ahead; a later or `null` request gets seven days
//!   ([`TouchOutcome::Applied`]). Expiry is a hint only: nothing is
//!   collected.
//! - The sha-256 digest of each whole blob is stored at creation and served
//!   by [`stored_digest`](BlobBackend::stored_digest); other digests are
//!   computed by the handlers.
//! - The default account capability ([`MemoryBackend::default_capability`]):
//!   `maxDataSources` 64, digests `sha-256` and `sha`, no lookup types,
//!   `application/x-tar` archive and extract, every other conversion
//!   `null` (decision 13).
//! - Objects that reference blobs exist only through
//!   `add_reference_for_test` (feature `test-helpers`): this backend stores
//!   no other data types, so without them `Blob/lookup` finds nothing,
//!   which is correct.
//!
//! The caller is `()`: every caller sees every blob of an account.
//!
//! [`MemoryBackend::default_capability`]: crate::memory::MemoryBackend::default_capability
//! [`MemoryBackend::register_account`]: crate::memory::MemoryBackend::register_account
//! [`MemoryBackend::with_account`]: crate::memory::MemoryBackend::with_account

#![deny(clippy::await_holding_lock)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range;
use std::sync::{Arc, Mutex, MutexGuard};

use jmap_blob_types::{BlobAccountCapability, DigestAlgorithm, errors};
use jmap_server::{
    BackendChangesError, BackendQueryError, BackendSetError, ChangesParams, ChangesResult,
    GetObject, JmapBackend, JmapObject, QueryChangesParams, QueryChangesResult, QueryObject,
    QueryParams, QueryResult, utc_date_from_epoch_seconds,
};
use jmap_types::{Id, SetError, SetErrorType, State, UTCDate};

use crate::backend::{
    BlobBackend, BlobCreate, BlobMeta, BlobSource, ChunkRef, ConvertOutcome, ConvertParams,
    CreatedBlob, DigestQuery, TouchOutcome,
};

/// Lifetime of a new blob: one hour.
const EXPIRES_SECS: i64 = 60 * 60;
/// Lifetime of a new `noPersist` blob: five minutes.
const NO_PERSIST_EXPIRES_SECS: i64 = 5 * 60;
/// The furthest ahead a touch is honoured: seven days.
const MAX_TOUCH_SECS: i64 = 7 * 24 * 60 * 60;

/// How a blob's octets are held.
#[derive(Debug, Clone)]
enum Repr {
    /// The octets themselves.
    Bytes(Vec<u8>),
    /// Ranges of leaf (`Bytes`) blobs of the same account, in order.
    Chunks(Vec<ChunkRef>),
}

#[derive(Debug, Clone)]
struct StoredBlob {
    repr: Repr,
    size: u64,
    media_type: Option<String>,
    /// Seconds since the epoch, or `None` for no specific expiry.
    expires: Option<i64>,
    /// Base64 sha-256 of the whole blob.
    sha256: Option<String>,
}

#[derive(Debug)]
struct Account {
    capability: BlobAccountCapability,
    blobs: HashMap<Id, StoredBlob>,
    /// `(type name, object id)` → blobs that object references.
    references: BTreeMap<(String, Id), BTreeSet<Id>>,
    /// Objects the caller cannot discover (left out of Blob/lookup).
    hidden: BTreeSet<(String, Id)>,
}

impl Account {
    fn new() -> Self {
        Self {
            capability: MemoryBackend::default_capability(),
            blobs: HashMap::new(),
            references: BTreeMap::new(),
            hidden: BTreeSet::new(),
        }
    }

    fn is_referenced(&self, blob_id: &Id) -> bool {
        self.references
            .values()
            .any(|blobs| blobs.contains(blob_id))
    }

    fn is_chunk_of_another(&self, blob_id: &Id) -> bool {
        self.blobs.iter().any(|(id, b)| {
            id != blob_id
                && matches!(&b.repr, Repr::Chunks(c) if c.iter().any(|r| &r.blob_id == blob_id))
        })
    }

    /// Leaf chunks covering `range` of `blob_id`, or `None` if the blob is
    /// missing or `range` runs past its end.
    fn leaf_chunks(&self, blob_id: &Id, range: Range<u64>) -> Option<Vec<ChunkRef>> {
        let blob = self.blobs.get(blob_id)?;
        if range.start > range.end || range.end > blob.size {
            return None;
        }
        match &blob.repr {
            Repr::Bytes(_) => Some(if range.is_empty() {
                Vec::new()
            } else {
                vec![ChunkRef::new(
                    blob_id.clone(),
                    range.start,
                    range.end - range.start,
                    blob.size,
                )]
            }),
            Repr::Chunks(chunks) => {
                let mut out = Vec::new();
                let mut pos = 0u64;
                for c in chunks {
                    let (lo, hi) = (pos, pos + c.length);
                    let (from, to) = (range.start.max(lo), range.end.min(hi));
                    if from < to {
                        out.push(ChunkRef::new(
                            c.blob_id.clone(),
                            c.offset + (from - lo),
                            to - from,
                            c.size,
                        ));
                    }
                    pos = hi;
                }
                Some(out)
            }
        }
    }

    /// The octets `range` of `blob_id`, or `None` if missing or out of
    /// range.
    fn read(&self, blob_id: &Id, range: Range<u64>) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        for c in self.leaf_chunks(blob_id, range)? {
            let Repr::Bytes(bytes) = &self.blobs.get(&c.blob_id)?.repr else {
                return None;
            };
            out.extend_from_slice(bytes.get(to_usize(c.offset)?..to_usize(c.offset + c.length)?)?);
        }
        Some(out)
    }
}

fn to_usize(n: u64) -> Option<usize> {
    usize::try_from(n).ok()
}

#[derive(Debug)]
struct Inner {
    accounts: HashMap<Id, Account>,
    next_id: u64,
    /// Fixed clock (seconds since the epoch), or `None` for the system
    /// clock.
    now: Option<i64>,
    /// Leave `noPersist` blobs out of the response.
    omit_no_persist: bool,
}

impl Inner {
    fn mint(&mut self) -> Id {
        self.next_id += 1;
        Id::from(format!("B{}", self.next_id))
    }

    fn now(&self) -> i64 {
        self.now.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .and_then(|d| i64::try_from(d.as_secs()).ok())
                .unwrap_or(0)
        })
    }
}

/// In-memory blob storage. Cloning shares the storage.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MemoryBackend {
    inner: Arc<Mutex<Inner>>,
}

impl Default for MemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// The error type of [`MemoryBackend`].
#[non_exhaustive]
#[derive(Debug)]
pub struct MemoryError {
    description: String,
}

impl MemoryError {
    fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
        }
    }

    /// What went wrong.
    pub fn description(&self) -> &str {
        &self.description
    }
}

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MemoryBackend error: {}", self.description)
    }
}

impl std::error::Error for MemoryError {}

fn set_error(t: SetErrorType, description: impl Into<String>) -> BackendSetError<MemoryError> {
    BackendSetError::SetError(Box::new(SetError::new(t).with_description(description)))
}

fn not_found(blob_id: &Id) -> BackendSetError<MemoryError> {
    set_error(
        SetErrorType::NotFound,
        format!("blob {} not found", blob_id.as_ref()),
    )
}

fn unknown_account(account_id: &Id) -> MemoryError {
    MemoryError::new(format!("unknown account {}", account_id.as_ref()))
}

/// The stored sha-256 of `bytes`; `None` only if no built-in digester
/// exists, and then the handlers compute it.
fn sha256(bytes: &[u8]) -> Option<String> {
    let mut d = DigestAlgorithm::Sha256.builtin_digester()?;
    d.update(bytes);
    Some(d.finish())
}

impl MemoryBackend {
    /// An empty backend with no accounts.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                accounts: HashMap::new(),
                next_id: 0,
                now: None,
                omit_no_persist: false,
            })),
        }
    }

    /// Add an empty account with the default capability.
    #[must_use]
    pub fn with_account(self, account_id: &Id) -> Self {
        self.register_account(account_id);
        self
    }

    /// Add an empty account with the default capability; an existing
    /// account is left as it is.
    pub fn register_account(&self, account_id: &Id) {
        self.lock()
            .accounts
            .entry(account_id.clone())
            .or_insert_with(Account::new);
    }

    /// The capability every new account advertises (decision 13):
    /// `maxDataSources` 64, `supportedDigestAlgorithms` `["sha-256",
    /// "sha"]`, no `supportedTypeNames`, `application/x-tar` for archive
    /// and extract, and every other list and limit `null`.
    pub fn default_capability() -> BlobAccountCapability {
        let tar = || Some(vec!["application/x-tar".to_owned()]);
        let mut cap = BlobAccountCapability::default();
        cap.supported_digest_algorithms = vec![DigestAlgorithm::Sha256, DigestAlgorithm::Sha];
        cap.supported_archive_types = tar();
        cap.supported_extract_types = tar();
        cap
    }

    /// Advertise `capability` for `account_id`, which must exist.
    ///
    /// # Panics
    ///
    /// If the account does not exist.
    #[cfg(feature = "test-helpers")]
    pub fn advertise_for_test(&self, account_id: &Id, capability: BlobAccountCapability) {
        self.with_account_mut(account_id, |a| a.capability = capability);
    }

    /// Record that object `object_id` of type `type_name` references
    /// `blob_id`: it then keeps the blob from being destroyed and shows in
    /// `Blob/lookup`.
    ///
    /// # Panics
    ///
    /// If the account does not exist.
    #[cfg(feature = "test-helpers")]
    pub fn add_reference_for_test(
        &self,
        account_id: &Id,
        type_name: &str,
        object_id: &Id,
        blob_id: &Id,
    ) {
        self.with_account_mut(account_id, |a| {
            a.references
                .entry((type_name.to_owned(), object_id.clone()))
                .or_default()
                .insert(blob_id.clone());
        });
    }

    /// Make object `object_id` of type `type_name` undiscoverable: its id
    /// no longer appears in `Blob/lookup`, though it still references its
    /// blobs.
    ///
    /// # Panics
    ///
    /// If the account does not exist.
    #[cfg(feature = "test-helpers")]
    pub fn hide_object_for_test(&self, account_id: &Id, type_name: &str, object_id: &Id) {
        self.with_account_mut(account_id, |a| {
            a.hidden.insert((type_name.to_owned(), object_id.clone()));
        });
    }

    /// Fix the clock at `epoch_seconds` for `expires` computations.
    #[cfg(feature = "test-helpers")]
    pub fn set_clock_for_test(&self, epoch_seconds: i64) {
        self.lock().now = Some(epoch_seconds);
    }

    /// Leave `noPersist` blobs out of `created` and `createdIds`, as
    /// blobext-03 §4 and §8 allow (§9.5 shows it). By default they are
    /// reported.
    #[cfg(feature = "test-helpers")]
    pub fn omit_no_persist_for_test(&self, omit: bool) {
        self.lock().omit_no_persist = omit;
    }

    #[cfg(feature = "test-helpers")]
    fn with_account_mut(&self, account_id: &Id, f: impl FnOnce(&mut Account)) {
        let mut inner = self.lock();
        let acc = inner
            .accounts
            .get_mut(account_id)
            .unwrap_or_else(|| panic!("unknown account {}", account_id.as_ref()));
        f(acc);
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A poisoned lock only means another test thread panicked; the
        // data is still usable for a reference backend.
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn info(&self, account_id: &Id, blob_id: &Id) -> Result<Option<BlobMeta>, MemoryError> {
        let inner = self.lock();
        let acc = inner
            .accounts
            .get(account_id)
            .ok_or_else(|| unknown_account(account_id))?;
        Ok(acc.blobs.get(blob_id).map(|b| {
            let mut meta = BlobMeta::new(b.media_type.clone(), b.size);
            meta.expires = if acc.is_referenced(blob_id) {
                None
            } else {
                b.expires.and_then(utc_date_from_epoch_seconds)
            };
            meta
        }))
    }

    fn create(
        &self,
        account_id: &Id,
        create: BlobCreate,
    ) -> Result<CreatedBlob, BackendSetError<MemoryError>> {
        let mut inner = self.lock();
        let now = inner.now();
        let omit = inner.omit_no_persist && create.no_persist;
        let Some(acc) = inner.accounts.get(account_id) else {
            return Err(BackendSetError::Other(unknown_account(account_id)));
        };

        // Re-check every blobId source and collect the pieces.
        enum Piece {
            Bytes(Vec<u8>),
            Chunks(Vec<ChunkRef>),
        }
        let mut pieces = Vec::new();
        let mut all_inline = true;
        for source in create.sources {
            match source {
                BlobSource::Bytes(b) => pieces.push(Piece::Bytes(b)),
                BlobSource::Blob { blob_id, range } => {
                    all_inline = false;
                    let chunks = acc
                        .leaf_chunks(&blob_id, range)
                        .ok_or_else(|| not_found(&blob_id))?;
                    pieces.push(Piece::Chunks(chunks));
                }
            }
        }

        let expires = now
            + if create.no_persist {
                NO_PERSIST_EXPIRES_SECS
            } else {
                EXPIRES_SECS
            };

        let (repr, bytes) = if all_inline {
            let bytes: Vec<u8> = pieces
                .into_iter()
                .flat_map(|p| match p {
                    Piece::Bytes(b) => b,
                    Piece::Chunks(_) => Vec::new(),
                })
                .collect();
            (Repr::Bytes(bytes.clone()), bytes)
        } else {
            // Inline pieces become leaf blobs of their own.
            let mut chunks = Vec::new();
            let mut new_leaves = Vec::new();
            for p in pieces {
                match p {
                    Piece::Chunks(c) => chunks.extend(c),
                    Piece::Bytes(b) if b.is_empty() => {}
                    Piece::Bytes(b) => {
                        let len = b.len() as u64;
                        let id = inner.mint();
                        chunks.push(ChunkRef::new(id.clone(), 0, len, len));
                        new_leaves.push((id, b));
                    }
                }
            }
            let Some(acc) = inner.accounts.get_mut(account_id) else {
                return Err(BackendSetError::Other(unknown_account(account_id)));
            };
            for (id, b) in new_leaves {
                acc.blobs.insert(
                    id,
                    StoredBlob {
                        size: b.len() as u64,
                        sha256: sha256(&b),
                        repr: Repr::Bytes(b),
                        media_type: None,
                        expires: Some(expires),
                    },
                );
            }
            let mut bytes = Vec::new();
            for c in &chunks {
                let Some(Repr::Bytes(b)) = acc.blobs.get(&c.blob_id).map(|b| &b.repr) else {
                    return Err(not_found(&c.blob_id));
                };
                let range = to_usize(c.offset).zip(to_usize(c.offset + c.length));
                let Some(slice) = range.and_then(|(a, z)| b.get(a..z)) else {
                    return Err(not_found(&c.blob_id));
                };
                bytes.extend_from_slice(slice);
            }
            (Repr::Chunks(chunks), bytes)
        };

        let size = bytes.len() as u64;
        let id = inner.mint();
        let Some(acc) = inner.accounts.get_mut(account_id) else {
            return Err(BackendSetError::Other(unknown_account(account_id)));
        };
        acc.blobs.insert(
            id.clone(),
            StoredBlob {
                repr,
                size,
                media_type: create.media_type.clone(),
                expires: Some(expires),
                sha256: sha256(&bytes),
            },
        );
        let mut created = CreatedBlob::new(id, create.media_type, size);
        created.expires = utc_date_from_epoch_seconds(expires);
        created.omit_from_response = omit;
        Ok(created)
    }

    fn touch(
        &self,
        account_id: &Id,
        blob_id: &Id,
        requested: Option<UTCDate>,
    ) -> Result<TouchOutcome, BackendSetError<MemoryError>> {
        let mut inner = self.lock();
        let limit = inner.now() + MAX_TOUCH_SECS;
        let Some(acc) = inner.accounts.get_mut(account_id) else {
            return Err(BackendSetError::Other(unknown_account(account_id)));
        };
        let blob = acc
            .blobs
            .get_mut(blob_id)
            .ok_or_else(|| not_found(blob_id))?;
        let wanted = match &requested {
            Some(d) => Some(d.to_epoch_seconds().map_err(|e| {
                set_error(SetErrorType::InvalidProperties, format!("expires: {e}"))
            })?),
            None => None,
        };
        match wanted {
            Some(t) if t <= limit => {
                blob.expires = Some(t);
                Ok(TouchOutcome::Honoured)
            }
            _ => {
                blob.expires = Some(limit);
                Ok(TouchOutcome::Applied(utc_date_from_epoch_seconds(limit)))
            }
        }
    }

    fn destroy(&self, account_id: &Id, blob_id: &Id) -> Result<(), BackendSetError<MemoryError>> {
        let mut inner = self.lock();
        let Some(acc) = inner.accounts.get_mut(account_id) else {
            return Err(BackendSetError::Other(unknown_account(account_id)));
        };
        if !acc.blobs.contains_key(blob_id) {
            return Err(not_found(blob_id));
        }
        if acc.is_referenced(blob_id) || acc.is_chunk_of_another(blob_id) {
            return Err(set_error(
                SetErrorType::from(errors::BLOB_HAS_REFERENCE),
                format!("blob {} is still referenced", blob_id.as_ref()),
            ));
        }
        acc.blobs.remove(blob_id);
        Ok(())
    }

    fn references(
        &self,
        account_id: &Id,
        type_name: &str,
        blob_ids: &[Id],
    ) -> Result<BTreeMap<Id, Vec<Id>>, MemoryError> {
        let inner = self.lock();
        let acc = inner
            .accounts
            .get(account_id)
            .ok_or_else(|| unknown_account(account_id))?;
        let mut out = BTreeMap::new();
        for blob_id in blob_ids {
            if !acc.blobs.contains_key(blob_id) {
                continue;
            }
            let ids: Vec<Id> = acc
                .references
                .iter()
                .filter(|((t, _), blobs)| t == type_name && blobs.contains(blob_id))
                .filter(|(key, _)| !acc.hidden.contains(*key))
                .map(|((_, object_id), _)| object_id.clone())
                .collect();
            out.insert(blob_id.clone(), ids);
        }
        Ok(out)
    }
}

fn not_an_object_store<T>(type_name: &str) -> Result<T, MemoryError> {
    Err(MemoryError::new(format!(
        "the blob MemoryBackend stores no {type_name} objects"
    )))
}

impl JmapBackend for MemoryBackend {
    type Error = MemoryError;
    type CallerCtx = ();

    async fn account_exists(&self, _caller: &(), account_id: &Id) -> Result<bool, MemoryError> {
        Ok(self.lock().accounts.contains_key(account_id))
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
        _ids: Option<&[Id]>,
        _properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), MemoryError> {
        not_an_object_store(O::TYPE_NAME)
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
    ) -> Result<State, MemoryError> {
        not_an_object_store(O::TYPE_NAME)
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
        _params: ChangesParams<'_>,
    ) -> Result<ChangesResult, BackendChangesError<MemoryError>> {
        not_an_object_store(O::TYPE_NAME).map_err(BackendChangesError::Other)
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
        _params: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<MemoryError>> {
        not_an_object_store(O::TYPE_NAME).map_err(BackendQueryError::Other)
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
        _params: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<MemoryError>> {
        not_an_object_store(O::TYPE_NAME).map_err(BackendChangesError::Other)
    }
}

impl BlobBackend for MemoryBackend {
    async fn blob_capability(
        &self,
        _caller: &(),
        account_id: &Id,
    ) -> Result<Option<BlobAccountCapability>, MemoryError> {
        Ok(self
            .lock()
            .accounts
            .get(account_id)
            .map(|a| a.capability.clone()))
    }

    async fn blob_info(
        &self,
        _caller: &(),
        account_id: &Id,
        blob_id: &Id,
    ) -> Result<Option<BlobMeta>, MemoryError> {
        self.info(account_id, blob_id)
    }

    async fn read_blob_range(
        &self,
        _caller: &(),
        account_id: &Id,
        blob_id: &Id,
        range: Range<u64>,
    ) -> Result<Option<Vec<u8>>, MemoryError> {
        let inner = self.lock();
        let acc = inner
            .accounts
            .get(account_id)
            .ok_or_else(|| unknown_account(account_id))?;
        Ok(acc.read(blob_id, range))
    }

    async fn stored_digest(
        &self,
        _caller: &(),
        account_id: &Id,
        query: &DigestQuery,
    ) -> Result<Option<String>, MemoryError> {
        if query.alg != DigestAlgorithm::Sha256 {
            return Ok(None);
        }
        let inner = self.lock();
        Ok(inner
            .accounts
            .get(account_id)
            .and_then(|a| a.blobs.get(&query.blob_id))
            .filter(|b| query.range.start == 0 && query.range.end == b.size)
            .and_then(|b| b.sha256.clone()))
    }

    async fn blob_chunks(
        &self,
        _caller: &(),
        account_id: &Id,
        blob_id: &Id,
    ) -> Result<Option<Vec<ChunkRef>>, MemoryError> {
        let inner = self.lock();
        let acc = inner
            .accounts
            .get(account_id)
            .ok_or_else(|| unknown_account(account_id))?;
        Ok(acc.blobs.get(blob_id).map(|b| match &b.repr {
            Repr::Chunks(c) => c.clone(),
            Repr::Bytes(_) => vec![ChunkRef::new(blob_id.clone(), 0, b.size, b.size)],
        }))
    }

    async fn create_blob(
        &self,
        _caller: &(),
        account_id: &Id,
        create: BlobCreate,
    ) -> Result<CreatedBlob, BackendSetError<MemoryError>> {
        self.create(account_id, create)
    }

    async fn touch_blob(
        &self,
        _caller: &(),
        account_id: &Id,
        blob_id: &Id,
        requested: Option<UTCDate>,
    ) -> Result<TouchOutcome, BackendSetError<MemoryError>> {
        self.touch(account_id, blob_id, requested)
    }

    async fn destroy_blob(
        &self,
        _caller: &(),
        account_id: &Id,
        blob_id: &Id,
    ) -> Result<(), BackendSetError<MemoryError>> {
        self.destroy(account_id, blob_id)
    }

    async fn blob_references(
        &self,
        _caller: &(),
        account_id: &Id,
        type_name: &str,
        blob_ids: &[Id],
    ) -> Result<BTreeMap<Id, Vec<Id>>, MemoryError> {
        self.references(account_id, type_name, blob_ids)
    }

    async fn convert(
        &self,
        _caller: &(),
        _account_id: &Id,
        _params: ConvertParams,
    ) -> Result<ConvertOutcome, BackendSetError<MemoryError>> {
        // FIXME(bd JMAP-tna1.2.6): ustar archive/extract and deferred
        // results. Until then a conversion is a serverFail, never a
        // misleading conversionFailed.
        Err(BackendSetError::Other(MemoryError::new(
            "Blob/convert is not implemented in the memory backend yet",
        )))
    }
}
