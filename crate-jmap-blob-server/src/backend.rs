//! The [`BlobBackend`] trait and the values its hooks take and return
//! (docs/design/blobext.md §6).
//!
//! Every hook takes the caller and the account first, so a backend applies
//! its own access rules: a blob the caller may not see is reported as
//! missing, never as forbidden. Hooks without a default have none because
//! any default would put wrong data on the wire (workspace "Default bodies
//! on spec-semantic backend methods").

use std::collections::BTreeMap;
use std::ops::Range;

use jmap_blob_types::{
    ArchiveEntry, BlobAccountCapability, Digest, DigestAlgorithm, ImageData, Recipe,
};
use jmap_server::{BackendSetError, JmapBackend};
use jmap_types::data_types::data_type;
use jmap_types::{Id, UTCDate};

/// Storage for blobs: the hooks behind `Blob/get`, `Blob/set`,
/// `Blob/lookup` and `Blob/convert` (draft-ietf-jmap-blobext-03), the
/// RFC 9404 methods and RFC 8620 §6.3 `Blob/copy`.
///
/// The handlers own argument validation, `#creationId` resolution, range
/// selection, digest computation and error mapping; the backend owns the
/// octets, the reverse reference index and any conversion. A hook that
/// returns `Err(Self::Error)` (or `BackendSetError::Other`) becomes
/// `serverFail`; return it for transient failures only, never to mean
/// "not found".
///
/// There is no atomic Blob/set hook: an atomic Blob/set runs inside the
/// dispatcher's conditional scope (decisions 1 and 17).
pub trait BlobBackend: JmapBackend {
    /// The value of `urn:ietf:params:jmap:blob2` in `account_id`'s
    /// `accountCapabilities` (blobext-03 §2.1), or `None` when the account
    /// does not support blob2 (`accountNotSupportedByMethod`).
    ///
    /// The handlers enforce its limits (`maxDataSources`,
    /// `maxSizeBlobSet`, `maxConvertSize`, ...) and accept only its
    /// `supportedDigestAlgorithms`, `supportedTypeNames` and conversion
    /// types. For RFC 9404 the Session carries its
    /// [`rfc9404`](BlobAccountCapability::rfc9404) view.
    ///
    /// No default: a default would advertise limits nobody enforces.
    fn blob_capability(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
    ) -> impl std::future::Future<Output = Result<Option<BlobAccountCapability>, Self::Error>> + Send;

    /// Type, size and expiry of `blob_id`, or `None` when it does not exist
    /// or the caller may not see it.
    ///
    /// For a deferred `Blob/convert` result this MUST generate the blob:
    /// "If a client later requests the size property via Blob/get for a
    /// deferred blob, the server MUST generate the blob at that point"
    /// (blobext-03 §8). If generation fails, return `Ok(None)`: the blob
    /// then does not exist everywhere (§8).
    ///
    /// No default.
    fn blob_info(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        blob_id: &Id,
    ) -> impl std::future::Future<Output = Result<Option<BlobMeta>, Self::Error>> + Send;

    /// Exactly the octets `range` of `blob_id`, or `None` when the blob does
    /// not exist or the caller may not see it.
    ///
    /// The handler has already clamped `range` to the blob's size
    /// (blobext-03 §5, `isTruncated`). A deferred blob is generated first,
    /// as in [`blob_info`](Self::blob_info).
    ///
    /// No default.
    fn read_blob_range(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        blob_id: &Id,
        range: Range<u64>,
    ) -> impl std::future::Future<Output = Result<Option<Vec<u8>>, Self::Error>> + Send;

    /// A stored digest of exactly `query.range` of `query.blob_id` with
    /// `query.alg`, in padded base64, or `None` to let the handler compute
    /// it.
    ///
    /// blobext-03 §2.1: a server "that stores a digest of each blob ... can
    /// return that one without reading the blob". A value returned here
    /// MUST be correct (§3: "it MUST calculate them correctly").
    ///
    /// Default: `Ok(None)`, so the handler computes every digest.
    fn stored_digest(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        query: &DigestQuery,
    ) -> impl std::future::Future<Output = Result<Option<String>, Self::Error>> + Send {
        let _ = (caller, account_id, query);
        async { Ok(None) }
    }

    /// A streaming digester for `alg`, used when
    /// [`stored_digest`](Self::stored_digest) returns `None`.
    ///
    /// Default: [`DigestAlgorithm::builtin_digester`] (sha, sha-256,
    /// sha-512). For an advertised algorithm with no digester the handler
    /// answers `serverFail`, never a wrong or empty value; override this to
    /// compute the others.
    fn digester(&self, alg: &DigestAlgorithm) -> Option<Box<dyn Digest + Send>> {
        alg.builtin_digester()
    }

    /// The stored chunk structure of `blob_id` (blobext-03 §5 `chunks`),
    /// or `None` when the blob does not exist or the caller may not see it.
    ///
    /// The handler projects each [`ChunkRef`] to the requested
    /// `dataSourceProperties` and computes `position` and chunk digests.
    ///
    /// Default: one chunk covering the whole blob, from
    /// [`blob_info`](Self::blob_info): "A server that does not store the
    /// blob in chunks returns a single DataSourceObject covering the entire
    /// blob, with blobId set to the blob's own id, offset 0, length and
    /// size equal to the size of the blob, and position 0" (§5).
    fn blob_chunks(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        blob_id: &Id,
    ) -> impl std::future::Future<Output = Result<Option<Vec<ChunkRef>>, Self::Error>> + Send {
        async move {
            Ok(self
                .blob_info(caller, account_id, blob_id)
                .await?
                .map(|meta| vec![ChunkRef::new(blob_id.clone(), 0, meta.size, meta.size)]))
        }
    }

    /// Image or video metadata of `blob_id` (blobext-03 §5 `imageData`), or
    /// `None` when the server cannot extract it.
    ///
    /// Called only for a blob [`blob_info`](Self::blob_info) found.
    ///
    /// Default: `Ok(None)`, written as `null` ("Otherwise null").
    fn image_data(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        blob_id: &Id,
    ) -> impl std::future::Future<Output = Result<Option<ImageData>, Self::Error>> + Send {
        let _ = (caller, account_id, blob_id);
        async { Ok(None) }
    }

    /// Create a blob by concatenating `create.sources` (blobext-03 §4).
    ///
    /// The handler has checked the sources against the capability, the
    /// ranges, sizes, positions and digests. The backend, atomically:
    /// re-checks that every source blob still exists for the caller
    /// (`notFound`), applies its quota (`overQuota`), may scan the content
    /// (§10.5), mints the id, and stores the octets (it may store a chunk
    /// list instead of copying). Return SetErrors as
    /// [`BackendSetError::SetError`].
    ///
    /// [`CreatedBlob::omit_from_response`] may be true only for a
    /// `no_persist` create that left no referenceable blob (§4).
    ///
    /// No default.
    fn create_blob(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        create: BlobCreate,
    ) -> impl std::future::Future<Output = Result<CreatedBlob, BackendSetError<Self::Error>>> + Send;

    /// "Touch" `blob_id`, asking that it live until `requested` (`None`:
    /// no specific expiry) (blobext-03 §4 update, §7).
    ///
    /// Return [`TouchOutcome::Honoured`] when the requested value was
    /// applied, or [`TouchOutcome::Applied`] with the value actually
    /// applied. A missing or invisible blob is a `notFound` SetError.
    ///
    /// No default.
    fn touch_blob(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        blob_id: &Id,
        requested: Option<UTCDate>,
    ) -> impl std::future::Future<Output = Result<TouchOutcome, BackendSetError<Self::Error>>> + Send;

    /// Destroy `blob_id` (blobext-03 §4 destroy).
    ///
    /// SetErrors: `notFound` when missing or invisible; `blobHasReference`
    /// ([`jmap_blob_types::errors::BLOB_HAS_REFERENCE`]) when a current
    /// object still references it ("The server MUST reject any blob that is
    /// still referenced by another object"); non-current object-history
    /// versions do not count. `forbidden` when the caller may see but not
    /// destroy it.
    ///
    /// No default.
    fn destroy_blob(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        blob_id: &Id,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Copy `blob_id` from `from_account_id` into `to_account_id` (RFC 8620
    /// §6.3 `Blob/copy`) and return the blob's id there.
    ///
    /// The handler has checked that both accounts exist and differ. The
    /// backend checks the caller may read the blob in `from_account_id` and
    /// write to `to_account_id`, applies its quota, and stores the octets
    /// (it may share them). The copy keeps the source's media type and
    /// starts unreferenced, so it expires as any new blob does.
    ///
    /// SetErrors, as [`BackendSetError::SetError`]: `notFound` when the
    /// blob is missing or the caller may not see it ("the "notFound"
    /// SetError error may be returned if the blobId to be copied cannot be
    /// found"), and "any of the standard set errors that may be returned
    /// for a create" (`forbidden`, `overQuota`, `tooLarge`, ...).
    ///
    /// Called for an account with or without a blob capability: Blob/copy
    /// is core (`urn:ietf:params:jmap:core`).
    ///
    /// No default: a no-op default would report copies that were never
    /// made.
    fn copy_blob(
        &self,
        caller: &Self::CallerCtx,
        from_account_id: &Id,
        to_account_id: &Id,
        blob_id: &Id,
    ) -> impl std::future::Future<Output = Result<Id, BackendSetError<Self::Error>>> + Send;

    /// The capability URI of `type_name` when it is a type that can
    /// reference blobs, or `None` for an unknown type or one that cannot
    /// (blobext-03 §6: valid names are those "whose "JMAP Data Types"
    /// registry entry has "Can Reference Blobs" set to "Yes" (or private
    /// types defined to reference blobs)").
    ///
    /// `Blob/lookup` answers `unknownDataType` for `None`, and for a URI the
    /// request's `using` lacks.
    ///
    /// Default: the registry, [`jmap_types::data_types`], filtered on
    /// `can_reference_blobs`. Override it to add private types.
    fn lookup_type_capability(&self, type_name: &str) -> Option<&str> {
        data_type(type_name)
            .filter(|e| e.can_reference_blobs)
            .map(|e| e.capability)
    }

    /// For each of `blob_ids`, the ids of `type_name` objects that
    /// reference it (blobext-03 §6).
    ///
    /// "Reference" includes containment: a Mailbox or Thread references the
    /// blobs of the Emails it contains. Return only ids the caller "could
    /// discover by querying", current versions only; leave out (or map to
    /// an empty list) a blob the caller cannot see. The handler fills an
    /// empty list for every blob with no entry.
    ///
    /// No default: an empty default would deny real references.
    fn blob_references(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        type_name: &str,
        blob_ids: &[Id],
    ) -> impl std::future::Future<Output = Result<BTreeMap<Id, Vec<Id>>, Self::Error>> + Send;

    /// Run, or defer, the conversion `params.recipe` (blobext-03 §8).
    ///
    /// The handler has checked the recipe against the capability and
    /// resolved every `#creationId` reference, so each blobId in the recipe
    /// is a literal id. The backend sniffs input formats (`unknownFormat`
    /// when it cannot tell or does not support one), clamps a compression
    /// `level` to the format's range, guards against decompression bombs
    /// (`tooLarge`), may scan output (§10.5), and returns `conversionFailed`
    /// ([`jmap_blob_types::errors::CONVERSION_FAILED`]) when no usable output
    /// was produced. A deferred result has `size: None` and "MUST be usable
    /// in all contexts where a regular blobId is accepted".
    ///
    /// A backend whose capability advertises every conversion list as
    /// `null` is never called.
    ///
    /// No default: a default `conversionFailed` would misreport an
    /// advertised conversion.
    fn convert(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        params: ConvertParams,
    ) -> impl std::future::Future<Output = Result<ConvertOutcome, BackendSetError<Self::Error>>> + Send;
}

/// What [`BlobBackend::blob_info`] reports about a blob.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobMeta {
    /// The media type, or `None` if unknown.
    pub media_type: Option<String>,
    /// The size in octets.
    pub size: u64,
    /// When the blob is likely to stop being available (blobext-03 §7), or
    /// `None` for no specific expiry.
    pub expires: Option<UTCDate>,
}

impl BlobMeta {
    /// A blob of `size` octets with the given type and no expiry.
    pub fn new(media_type: Option<String>, size: u64) -> Self {
        Self {
            media_type,
            size,
            expires: None,
        }
    }
}

/// The digest [`BlobBackend::stored_digest`] is asked for.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DigestQuery {
    /// The blob.
    pub blob_id: Id,
    /// The algorithm.
    pub alg: DigestAlgorithm,
    /// The octets the digest covers (already clamped to the blob).
    pub range: Range<u64>,
}

impl DigestQuery {
    /// A query for the `alg` digest of `range` of `blob_id`.
    pub fn new(blob_id: Id, alg: DigestAlgorithm, range: Range<u64>) -> Self {
        Self {
            blob_id,
            alg,
            range,
        }
    }
}

/// One stored chunk of a blob (blobext-03 §5 `chunks`): the octets
/// `offset .. offset + length` of chunk blob `blob_id`, whose full size is
/// `size`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkRef {
    /// The chunk blob.
    pub blob_id: Id,
    /// Start of the used range within the chunk blob.
    pub offset: u64,
    /// Octets used from the chunk blob.
    pub length: u64,
    /// Full size of the chunk blob.
    pub size: u64,
}

impl ChunkRef {
    /// A chunk using `length` octets from `offset` of `blob_id`, which is
    /// `size` octets long.
    pub fn new(blob_id: Id, offset: u64, length: u64, size: u64) -> Self {
        Self {
            blob_id,
            offset,
            length,
            size,
        }
    }
}

/// One source of a new blob, after the handler resolved and checked it.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlobSource {
    /// Inline octets (decoded `data:asText` or `data:asBase64`).
    Bytes(Vec<u8>),
    /// The octets `range` of an existing blob.
    Blob {
        /// The source blob (a literal id: references are resolved).
        blob_id: Id,
        /// The octets taken from it.
        range: Range<u64>,
    },
}

/// A blob for [`BlobBackend::create_blob`] to create.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobCreate {
    /// The sources, concatenated in order.
    pub sources: Vec<BlobSource>,
    /// The client's media type hint (`type`).
    pub media_type: Option<String>,
    /// Total size of the new blob: the sum of the sources' lengths.
    pub size: u64,
    /// `noPersist`: the blob need only last for the request.
    pub no_persist: bool,
}

impl BlobCreate {
    /// A persistent blob from `sources`, `size` octets long.
    pub fn new(sources: Vec<BlobSource>, size: u64) -> Self {
        Self {
            sources,
            media_type: None,
            size,
            no_persist: false,
        }
    }
}

/// What [`BlobBackend::create_blob`] created.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedBlob {
    /// The new blobId.
    pub id: Id,
    /// The media type the server settled on (it MAY override the hint).
    pub media_type: Option<String>,
    /// The size in octets.
    pub size: u64,
    /// When the blob is likely to stop being available, or `None`.
    pub expires: Option<UTCDate>,
    /// Leave the blob out of `created` and `createdIds`: only for a
    /// `noPersist` create that left no referenceable blob (blobext-03 §4).
    pub omit_from_response: bool,
}

impl CreatedBlob {
    /// A created blob with no expiry, reported in the response.
    pub fn new(id: Id, media_type: Option<String>, size: u64) -> Self {
        Self {
            id,
            media_type,
            size,
            expires: None,
            omit_from_response: false,
        }
    }
}

/// The result of [`BlobBackend::touch_blob`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TouchOutcome {
    /// The requested `expires` was applied: `updated[id]` is `null`.
    Honoured,
    /// A different `expires` was applied: `updated[id]` reports it.
    Applied(Option<UTCDate>),
}

/// A conversion for [`BlobBackend::convert`] to run.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertParams {
    /// The recipe, every blobId a literal id.
    pub recipe: Recipe,
    /// `noPersist`: the result need only last for the request (no effect
    /// on an extract, which creates no blob).
    pub no_persist: bool,
}

impl ConvertParams {
    /// A persistent conversion running `recipe`.
    pub fn new(recipe: Recipe) -> Self {
        Self {
            recipe,
            no_persist: false,
        }
    }
}

/// What [`BlobBackend::convert`] produced.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertOutcome {
    /// The result blob; for an extract, the archive's own id.
    pub id: Id,
    /// Its media type.
    pub media_type: Option<String>,
    /// Its size, or `None` for a deferred result not yet generated.
    pub size: Option<u64>,
    /// When it is likely to stop being available, or `None`.
    pub expires: Option<UTCDate>,
    /// `isIncomplete`: the conversion did not complete cleanly.
    pub is_incomplete: bool,
    /// `description` of the problem, if any.
    pub description: Option<String>,
    /// `entries` of an extracted archive (extract only).
    pub entries: Option<Vec<ArchiveEntry>>,
    /// Leave the result out of `created` and `createdIds`: only for a
    /// `noPersist` conversion that left no referenceable blob (§8).
    pub omit_from_response: bool,
}

impl ConvertOutcome {
    /// A complete result with the given id, type and size.
    pub fn new(id: Id, media_type: Option<String>, size: Option<u64>) -> Self {
        Self {
            id,
            media_type,
            size,
            expires: None,
            is_incomplete: false,
            description: None,
            entries: None,
            omit_from_response: false,
        }
    }
}
