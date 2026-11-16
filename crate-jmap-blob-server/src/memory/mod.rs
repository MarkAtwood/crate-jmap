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
//! - Conversions (decision 13): an `application/x-tar` ArchiveRecipe is
//!   checked when `Blob/convert` runs and generated on first read (its
//!   `size` is `null` until then, blobext-03 §8); an `application/x-tar`
//!   ExtractRecipe lists the archive and gives each file a new blob. A
//!   failed generation (an entry blob destroyed meanwhile, or
//!   `fail_deferred_for_test`) removes the blob. Every other recipe runs
//!   through `stub_converter_for_test` (feature `test-helpers`), and is a
//!   `serverFail` without one.
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

mod convert;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range;
use std::sync::Arc;

use jmap_blob_types::{BlobAccountCapability, BlobRef, DigestAlgorithm, Recipe, errors};
use jmap_server::{
    BackendChangesError, BackendQueryError, BackendSetError, BeginSetError, ChangesParams,
    ChangesResult, ConditionalBackend, GetObject, JmapBackend, JmapObject, QueryChangesParams,
    QueryChangesResult, QueryObject, QueryParams, QueryResult, ScopedGuard, ScopedState, SetScope,
    SetScopeRequest, utc_date_from_epoch_seconds,
};
use jmap_types::{Id, SetError, SetErrorType, State, UTCDate};

use self::convert::{TarEntry, build_tar, looks_like_tar, parse_tar, plan_archive};
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
    /// An ArchiveRecipe result not generated yet (blobext-03 §8): built on
    /// first read. `size` is meaningless until then.
    Deferred(Deferred),
}

#[derive(Debug, Clone)]
struct Deferred {
    entries: Vec<TarEntry>,
    /// Make generation fail (`fail_deferred_for_test`).
    fail: bool,
}

/// Archive media type the memory backend builds and lists.
const TAR: &str = "application/x-tar";

#[derive(Debug, Clone)]
struct StoredBlob {
    repr: Repr,
    size: u64,
    media_type: Option<String>,
    /// Seconds since the epoch, or `None` for no specific expiry.
    expires: Option<i64>,
    /// Base64 sha-256 of the whole blob (`None` while deferred).
    sha256: Option<String>,
}

#[derive(Debug, Clone)]
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

    /// Generate `blob_id` if it is a deferred result. `false` when the blob
    /// does not exist, or its generation failed: it is then removed, so it
    /// "does not exist" everywhere (blobext-03 §8).
    fn materialize(&mut self, blob_id: &Id) -> bool {
        let plan = match self.blobs.get(blob_id) {
            None => return false,
            Some(StoredBlob {
                repr: Repr::Deferred(d),
                ..
            }) => d.clone(),
            Some(_) => return true,
        };
        let built = if plan.fail {
            None
        } else {
            for content in plan.entries.iter().filter_map(|e| e.content.as_ref()) {
                self.materialize(content);
            }
            build_tar(&plan.entries, |id| {
                let size = self.blobs.get(id)?.size;
                self.read(id, 0..size)
            })
        };
        match (built, self.blobs.get_mut(blob_id)) {
            (Some(bytes), Some(blob)) => {
                blob.size = bytes.len() as u64;
                blob.sha256 = sha256(&bytes);
                blob.repr = Repr::Bytes(bytes);
                true
            }
            _ => {
                self.blobs.remove(blob_id);
                false
            }
        }
    }

    /// Leaf chunks covering `range` of `blob_id`, or `None` if the blob is
    /// missing, not generated yet, or `range` runs past its end.
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
            Repr::Deferred(_) => None,
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

#[derive(Debug, Clone)]
struct Inner {
    accounts: HashMap<Id, Account>,
    next_id: u64,
    /// Fixed clock (seconds since the epoch), or `None` for the system
    /// clock.
    now: Option<i64>,
    /// Leave `noPersist` blobs out of the response.
    omit_no_persist: bool,
    /// Runs the recipes the backend does not implement itself.
    stub: Option<StubConverter>,
}

/// A test converter for the recipes the memory backend does not run
/// itself (`stub_converter_for_test`).
#[derive(Clone)]
struct StubConverter(Arc<ConvertFn>);

type ConvertFn = dyn Fn(&Recipe) -> Result<Vec<u8>, Box<SetError>> + Send + Sync;

impl std::fmt::Debug for StubConverter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StubConverter")
    }
}

fn mint(next_id: &mut u64) -> Id {
    *next_id += 1;
    Id::from(format!("B{next_id}"))
}

impl Inner {
    fn mint(&mut self) -> Id {
        mint(&mut self.next_id)
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
    inner: ScopedState<Inner>,
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
            inner: ScopedState::new(Inner {
                accounts: HashMap::new(),
                next_id: 0,
                now: None,
                omit_no_persist: false,
                stub: None,
            }),
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

    /// Run every recipe other than an `application/x-tar` archive or
    /// extract through `converter`, which returns the result's octets or a
    /// SetError (decision 13). Without one, such a recipe is a
    /// `serverFail`: the default capability advertises them as `null`.
    #[cfg(feature = "test-helpers")]
    pub fn stub_converter_for_test(
        &self,
        converter: impl Fn(&Recipe) -> Result<Vec<u8>, Box<SetError>> + Send + Sync + 'static,
    ) {
        self.lock().stub = Some(StubConverter(Arc::new(converter)));
    }

    /// Make the generation of deferred result `blob_id` fail: from then on
    /// the blob does not exist (blobext-03 §8).
    ///
    /// # Panics
    ///
    /// If the account does not exist or `blob_id` is not a deferred result
    /// that has not been generated yet.
    #[cfg(feature = "test-helpers")]
    pub fn fail_deferred_for_test(&self, account_id: &Id, blob_id: &Id) {
        self.with_account_mut(account_id, |a| match a.blobs.get_mut(blob_id) {
            Some(StoredBlob {
                repr: Repr::Deferred(d),
                ..
            }) => d.fail = true,
            _ => panic!("{} is not a pending deferred blob", blob_id.as_ref()),
        });
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

    /// The state, for sync setup and test helpers, which never run inside
    /// a Blob/set scope. A poisoned lock is taken anyway: the data is
    /// still usable for a reference backend.
    fn lock(&self) -> ScopedGuard<'_, Inner> {
        self.inner.lock_unscoped()
    }

    /// The state, for the backend methods: waits while another task's
    /// conditional Blob/set scope is open (docs/design/conditional.md §7).
    async fn locked(&self) -> ScopedGuard<'_, Inner> {
        self.inner.lock().await
    }

    fn info(
        inner: &mut Inner,
        account_id: &Id,
        blob_id: &Id,
    ) -> Result<Option<BlobMeta>, MemoryError> {
        let acc = inner
            .accounts
            .get_mut(account_id)
            .ok_or_else(|| unknown_account(account_id))?;
        if !acc.materialize(blob_id) {
            return Ok(None);
        }
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
        inner: &mut Inner,
        account_id: &Id,
        create: BlobCreate,
    ) -> Result<CreatedBlob, BackendSetError<MemoryError>> {
        let now = inner.now();
        let omit = inner.omit_no_persist && create.no_persist;
        let Some(acc) = inner.accounts.get_mut(account_id) else {
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
                    acc.materialize(&blob_id);
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
        inner: &mut Inner,
        account_id: &Id,
        blob_id: &Id,
        requested: Option<UTCDate>,
    ) -> Result<TouchOutcome, BackendSetError<MemoryError>> {
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

    fn destroy(
        inner: &mut Inner,
        account_id: &Id,
        blob_id: &Id,
    ) -> Result<(), BackendSetError<MemoryError>> {
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
        inner: &Inner,
        account_id: &Id,
        type_name: &str,
        blob_ids: &[Id],
    ) -> Result<BTreeMap<Id, Vec<Id>>, MemoryError> {
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

/// The blobIds a recipe reads.
fn recipe_inputs(recipe: &Recipe) -> Vec<&BlobRef> {
    match recipe {
        Recipe::ImageConvert(r) => vec![&r.blob_id],
        Recipe::Archive(r) => r
            .entries
            .iter()
            .filter_map(|e| e.blob_id.as_ref())
            .collect(),
        Recipe::Extract(r) => vec![&r.blob_id],
        Recipe::Compress(r) => vec![&r.blob_id],
        Recipe::Decompress(r) => vec![&r.blob_id],
        Recipe::Delta(r) => vec![&r.blob_id, &r.new_blob_id],
        Recipe::Patch(r) => vec![&r.blob_id, &r.delta_blob_id],
        _ => Vec::new(),
    }
}

/// The media type of a stub conversion's result, where the recipe names
/// one.
fn recipe_output_type(recipe: &Recipe) -> Option<String> {
    match recipe {
        Recipe::ImageConvert(r) => Some(r.media_type.clone()),
        Recipe::Archive(r) => Some(r.media_type.clone()),
        Recipe::Compress(r) => Some(r.media_type.clone()),
        Recipe::Delta(r) => Some(r.media_type.clone()),
        _ => None,
    }
}

fn literal(r: &BlobRef) -> Result<&Id, BackendSetError<MemoryError>> {
    r.as_id().ok_or_else(|| {
        BackendSetError::Other(MemoryError::new(format!(
            "unresolved creation reference {r}"
        )))
    })
}

impl MemoryBackend {
    fn convert_blob(
        guard: &mut Inner,
        account_id: &Id,
        params: ConvertParams,
    ) -> Result<ConvertOutcome, BackendSetError<MemoryError>> {
        let now = guard.now();
        let omit = guard.omit_no_persist && params.no_persist;
        let stub = guard.stub.clone();
        let Inner {
            accounts, next_id, ..
        } = &mut *guard;
        let Some(acc) = accounts.get_mut(account_id) else {
            return Err(BackendSetError::Other(unknown_account(account_id)));
        };
        let expires = now
            + if params.no_persist {
                NO_PERSIST_EXPIRES_SECS
            } else {
                EXPIRES_SECS
            };

        match &params.recipe {
            Recipe::Archive(r) if r.media_type.eq_ignore_ascii_case(TAR) => {
                let entries = plan_archive(r, now, |id| {
                    acc.materialize(id)
                        .then(|| acc.blobs.get(id).map(|b| b.size))
                        .flatten()
                })
                .map_err(BackendSetError::SetError)?;
                let id = mint(next_id);
                acc.blobs.insert(
                    id.clone(),
                    StoredBlob {
                        repr: Repr::Deferred(Deferred {
                            entries,
                            fail: false,
                        }),
                        size: 0,
                        media_type: Some(r.media_type.clone()),
                        expires: Some(expires),
                        sha256: None,
                    },
                );
                let mut out = ConvertOutcome::new(id, Some(r.media_type.clone()), None);
                out.expires = utc_date_from_epoch_seconds(expires);
                out.omit_from_response = omit;
                Ok(out)
            }
            Recipe::Extract(r) => {
                let archive = literal(&r.blob_id)?;
                if !acc.materialize(archive) {
                    return Err(not_found(archive));
                }
                let Some(blob) = acc.blobs.get(archive) else {
                    return Err(not_found(archive));
                };
                let (size, media_type) = (blob.size, blob.media_type.clone());
                let blob_expires = if acc.is_referenced(archive) {
                    None
                } else {
                    blob.expires
                };
                let bytes = acc
                    .read(archive, 0..size)
                    .ok_or_else(|| not_found(archive))?;
                let unknown = || {
                    set_error(
                        SetErrorType::from(errors::UNKNOWN_FORMAT),
                        "the memory backend extracts only application/x-tar",
                    )
                };
                match &r.media_type {
                    Some(t) if !t.eq_ignore_ascii_case(TAR) => return Err(unknown()),
                    None if !looks_like_tar(&bytes) => return Err(unknown()),
                    _ => {}
                }
                let parsed = parse_tar(&bytes).ok_or_else(|| {
                    set_error(
                        SetErrorType::from(errors::CONVERSION_FAILED),
                        "not a valid tar archive",
                    )
                })?;
                let mut entries = Vec::with_capacity(parsed.entries.len());
                for p in parsed.entries {
                    let mut entry = p.entry;
                    if let Some(content) = p.content {
                        let id = mint(next_id);
                        acc.blobs.insert(
                            id.clone(),
                            StoredBlob {
                                size: content.len() as u64,
                                sha256: sha256(&content),
                                repr: Repr::Bytes(content),
                                media_type: None,
                                expires: Some(now + EXPIRES_SECS),
                            },
                        );
                        entry.blob_id = Some(BlobRef::Id(id));
                    }
                    entries.push(entry);
                }
                // blobext-03 §8.4: the result's id, type and size are the
                // archive's; noPersist has no effect.
                let mut out = ConvertOutcome::new(archive.clone(), media_type, Some(size));
                out.expires = blob_expires.and_then(utc_date_from_epoch_seconds);
                out.is_incomplete = parsed.problem.is_some();
                out.description = parsed.problem;
                out.entries = Some(entries);
                Ok(out)
            }
            recipe => {
                let Some(StubConverter(convert)) = stub else {
                    return Err(BackendSetError::Other(MemoryError::new(format!(
                        "the memory backend does not run {} recipes",
                        recipe.key()
                    ))));
                };
                for input in recipe_inputs(recipe) {
                    let id = literal(input)?;
                    if !acc.materialize(id) {
                        return Err(not_found(id));
                    }
                }
                let bytes = convert(recipe).map_err(BackendSetError::SetError)?;
                let size = bytes.len() as u64;
                let media_type = recipe_output_type(recipe);
                let id = mint(next_id);
                acc.blobs.insert(
                    id.clone(),
                    StoredBlob {
                        size,
                        sha256: sha256(&bytes),
                        repr: Repr::Bytes(bytes),
                        media_type: media_type.clone(),
                        expires: Some(expires),
                    },
                );
                let mut out = ConvertOutcome::new(id, media_type, Some(size));
                out.expires = utc_date_from_epoch_seconds(expires);
                out.omit_from_response = omit;
                Ok(out)
            }
        }
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
        Ok(self.locked().await.accounts.contains_key(account_id))
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
            .locked()
            .await
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
        Self::info(&mut *self.locked().await, account_id, blob_id)
    }

    async fn read_blob_range(
        &self,
        _caller: &(),
        account_id: &Id,
        blob_id: &Id,
        range: Range<u64>,
    ) -> Result<Option<Vec<u8>>, MemoryError> {
        let mut inner = self.locked().await;
        let acc = inner
            .accounts
            .get_mut(account_id)
            .ok_or_else(|| unknown_account(account_id))?;
        if !acc.materialize(blob_id) {
            return Ok(None);
        }
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
        let inner = self.locked().await;
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
        let mut inner = self.locked().await;
        let acc = inner
            .accounts
            .get_mut(account_id)
            .ok_or_else(|| unknown_account(account_id))?;
        if !acc.materialize(blob_id) {
            return Ok(None);
        }
        Ok(acc.blobs.get(blob_id).map(|b| match &b.repr {
            Repr::Chunks(c) => c.clone(),
            Repr::Bytes(_) | Repr::Deferred(_) => {
                vec![ChunkRef::new(blob_id.clone(), 0, b.size, b.size)]
            }
        }))
    }

    async fn create_blob(
        &self,
        _caller: &(),
        account_id: &Id,
        create: BlobCreate,
    ) -> Result<CreatedBlob, BackendSetError<MemoryError>> {
        Self::create(&mut *self.locked().await, account_id, create)
    }

    async fn touch_blob(
        &self,
        _caller: &(),
        account_id: &Id,
        blob_id: &Id,
        requested: Option<UTCDate>,
    ) -> Result<TouchOutcome, BackendSetError<MemoryError>> {
        Self::touch(&mut *self.locked().await, account_id, blob_id, requested)
    }

    async fn destroy_blob(
        &self,
        _caller: &(),
        account_id: &Id,
        blob_id: &Id,
    ) -> Result<(), BackendSetError<MemoryError>> {
        Self::destroy(&mut *self.locked().await, account_id, blob_id)
    }

    async fn blob_references(
        &self,
        _caller: &(),
        account_id: &Id,
        type_name: &str,
        blob_ids: &[Id],
    ) -> Result<BTreeMap<Id, Vec<Id>>, MemoryError> {
        Self::references(&*self.locked().await, account_id, type_name, blob_ids)
    }

    async fn convert(
        &self,
        _caller: &(),
        account_id: &Id,
        params: ConvertParams,
    ) -> Result<ConvertOutcome, BackendSetError<MemoryError>> {
        Self::convert_blob(&mut *self.locked().await, account_id, params)
    }
}

/// Conditional Blob/set (draft-ietf-jmap-conditional-02, bd JMAP-tna1.2.12):
/// the whole store is one [`ScopedState`], so an atomic Blob/set's created
/// blobs, `noPersist` ones included, and its `expires` updates and
/// destroys are discarded on rollback. Blobs have no cross-object
/// constraint to check at commit.
impl ConditionalBackend for MemoryBackend {
    async fn begin_set(
        &self,
        _caller: &(),
        request: SetScopeRequest<'_>,
    ) -> Result<Box<dyn SetScope>, BeginSetError<MemoryError>> {
        Ok(self
            .inner
            .open(request, |_: &Inner, _: &Inner| Vec::new())
            .await)
    }
}
