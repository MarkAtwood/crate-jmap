//! `MetadataBackend` trait and supporting type re-exports.
//!
//! Consumers implement [`MetadataBackend`] for their storage system. The
//! method handlers in [`crate::metadata`] call into the backend through this
//! trait.
//!
//! Read-side operations (`get_objects`, `get_state`, `get_changes`,
//! `query_objects`, `query_changes`) are defined on the
//! [`jmap_server::JmapBackend`] supertrait. Only write operations live here.
//!
//! Property selector enum and `JmapObject`/`GetObject`/`SetObject`/`QueryObject`
//! impls for [`jmap_metadata_types::Metadata`] live in `jmap-metadata-types`
//! (`backend` module); they are re-exported here so a downstream
//! `MetadataBackend` impl can `use jmap_metadata_server::backend::*` and
//! get the full vocabulary — the foundation supertrait, the
//! [`MetadataProperty`] selector, and the standard wire error types — in
//! one import, without separately importing the foundation crate and
//! the types crate. The re-exported items are GROUPED here, not
//! redefined; their canonical home is the source crate.
//!
//! **Version pinning**: each `pub use` pins the upstream type's
//! major-version contract into this crate's public surface. A
//! SemVer-breaking change in [`jmap_server`] or
//! [`jmap_metadata_types`] cascades into a SemVer-breaking change
//! here. The workspace's path-dep coordination and synchronized
//! minor-version cadence (`0.1.x` across all `jmap-*` crates) is what
//! keeps this manageable.

pub use jmap_metadata_types::backend::MetadataProperty;
pub use jmap_server::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend,
    JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
};

// ---------------------------------------------------------------------------
// MetadataBackend trait
// ---------------------------------------------------------------------------

/// Storage backend for JMAP Object Metadata method handlers
/// (draft-ietf-jmap-metadata-01).
///
/// Implementors provide the actual data access; the method handlers in
/// [`crate::metadata`] translate between the JMAP wire protocol and these
/// backend calls.
///
/// Read-side operations (`get_objects`, `get_state`, `get_changes`,
/// `query_objects`, `query_changes`) are defined on the [`JmapBackend`]
/// supertrait. Only write operations and type introspection are here.
///
/// # Server-side semantic constraints (draft §3.1)
///
/// `Metadata/set` carries several server-side constraints the handler does
/// NOT enforce. Backends are responsible for reporting them via
/// [`BackendSetError::SetError`]:
///
/// - **Uniqueness** (§3.1): the (relatedType, relatedId, `@type`, isPrivate)
///   tuple MUST be unique within the user's view of the account. A duplicate
///   create returns `alreadyExists`; the SetError MAY carry an `existingId`
///   property pointing at the conflicting object.
/// - **`maySetPrivate` gating** (§1.2.1): if the account capability reports
///   `maySetPrivate: false` and the client supplies `isPrivate: true`, the
///   backend returns `forbidden`.
/// - **Quota** (§6): if the operation would exceed the account's metadata
///   quota, the backend returns `overQuota`.
/// - **Related-object validation** (§3.1): backends MUST verify the
///   `relatedType` is supported and the `relatedId` references an existing
///   object of that type; otherwise return `invalidProperties` listing
///   `relatedType` and/or `relatedId`.
///
/// This trait is not object-safe by design (generic methods). Use
/// `Arc<impl MetadataBackend>` when sharing across tasks.
pub trait MetadataBackend: JmapBackend {
    /// Create a new Metadata object.
    ///
    /// Returns `(assigned_id, created_object)` on success. `create_id` is the
    /// client-side creation id used in the `/set` request.
    ///
    /// # Invariant
    ///
    /// The returned `O` MUST have its `id` field set to the server-assigned
    /// [`Id`](jmap_types::Id) returned as the first element of the tuple.
    /// The handler relies on this to populate the `created` response map per
    /// RFC 8620 §5.3.
    ///
    /// # Errors
    ///
    /// See the trait-level "Server-side semantic constraints" notes for the
    /// full SetError catalogue the handler propagates back to clients.
    fn create_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        create_id: &str,
        obj: O,
    ) -> impl std::future::Future<Output = Result<(jmap_types::Id, O), BackendSetError<Self::Error>>>
           + Send;

    /// Apply a partial update (patch) to an existing Metadata object.
    ///
    /// Returns `Some(updated_object)` if the backend modified server-set
    /// properties beyond what the client requested (RFC 8620 §5.3 server-set
    /// field echo), or `None` if the patch was applied verbatim.
    ///
    /// # Errors
    ///
    /// An update that would change `relatedType`, `relatedId`, `@type`, or
    /// `isPrivate` such that the resulting object would conflict with another
    /// visible Metadata object MUST return `alreadyExists` per §3.1.
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: O::Patch,
    ) -> impl std::future::Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy a Metadata object by id.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Returns `true` if this backend implementation supports the given
    /// JMAP object type.
    ///
    /// # Contract
    ///
    /// This is a **global, stateless backend-capability check** — it asks
    /// "did this implementation wire up the methods needed for type
    /// `O`?", not "does this user's account have this type enabled?".
    /// That is why the method is synchronous and takes no `caller` or
    /// `account_id` arguments. Per-account capability variation belongs
    /// in the consumer's session-capability builder (workspace AGENTS.md
    /// library-kit posture), NOT here.
    ///
    /// # Types in scope
    ///
    /// Implementors should answer for the JMAP object types this trait
    /// covers (`Metadata` per draft-ietf-jmap-metadata-01). Backends MAY
    /// answer for object types from sibling extensions (`Email`,
    /// `Calendar`, etc.) if they also implement those traits, but the
    /// typical pattern is one backend impl per extension family. A
    /// backend that does not recognise `O` SHOULD return `false`.
    ///
    /// # Callers
    ///
    /// The server consumer calls this method when building the JMAP
    /// session capability response. The handler library does not call
    /// this method internally; per-account method-level errors are
    /// returned via `accountNotFound` / `accountNotSupportedByMethod` /
    /// `forbidden` from the individual /get / /set / /changes paths.
    ///
    /// # Default
    ///
    /// Backends that support Metadata unconditionally can return `true`
    /// always.
    fn supports_type<O: JmapObject>(&self) -> bool;

    /// Return changes since `since_state` filtered by the metadata-specific
    /// filter args from draft-ietf-jmap-metadata-01 §3.3 (`filterRelatedType`,
    /// `filterMetadataType`).
    ///
    /// Default impl delegates to [`JmapBackend::get_changes`], ignoring the
    /// filter args; the handler then post-filters `created` and `updated`
    /// by re-fetching objects via `get_objects::<Metadata>`. The `destroyed`
    /// array is returned unfiltered under the default impl — strict §3.3
    /// conformance requires overriding this method (see bd:JMAP-06zp.3.5.2).
    ///
    /// Backends that index their change log on `(relatedType, @type)` SHOULD
    /// override to pre-filter at the storage layer, which gains strict §3.3
    /// conformance for the `destroyed` array and removes the per-Id re-fetch
    /// cost for `created`/`updated`.
    ///
    /// The state token returned in `ChangesResult::new_state` is independent
    /// of the filter args per draft §3.3: a backend MUST NOT advance state
    /// based on filtered-out changes only. The handler does not enforce this
    /// — implementations honor the constraint themselves.
    fn get_metadata_changes(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        since_state: &jmap_types::State,
        max_changes: Option<u64>,
        filter_related_type: Option<&str>,
        filter_metadata_type: Option<&[String]>,
    ) -> impl std::future::Future<Output = Result<ChangesResult, BackendChangesError<Self::Error>>> + Send
    {
        let _ = (filter_related_type, filter_metadata_type);
        self.get_changes::<jmap_metadata_types::Metadata>(
            caller,
            account_id,
            since_state,
            max_changes,
        )
    }
}
