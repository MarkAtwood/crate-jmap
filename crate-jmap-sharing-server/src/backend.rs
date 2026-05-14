//! SharingBackend trait and supporting types for JMAP Sharing method handlers.
//!
//! Consumers implement [`SharingBackend`] for their storage system. The method
//! handlers in sibling modules call into the backend through this trait.
//!
//! The read-side operations (`get_objects`, `get_state`, `get_changes`,
//! `query_objects`, `query_changes`) are defined on the [`jmap_server::JmapBackend`]
//! supertrait. Only write operations are here.
//!
//! Marker traits and property selector enums live in `jmap-types` and
//! `jmap-sharing-types` respectively; they are re-exported here for convenience.

pub use jmap_server::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend,
    JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
};
pub use jmap_sharing_types::backend::{PrincipalProperty, ShareNotificationProperty};

// ---------------------------------------------------------------------------
// SharingBackend trait
// ---------------------------------------------------------------------------

/// Storage backend for JMAP Sharing method handlers (RFC 9670).
///
/// Implementors provide the actual data access; the method handler modules
/// in this crate translate between JMAP wire protocol and backend calls.
///
/// Read-side operations (`get_objects`, `get_state`, `get_changes`,
/// `query_objects`, `query_changes`) are defined on the [`JmapBackend`]
/// supertrait. Only write operations and type introspection are here.
///
/// This trait is not object-safe by design (generic methods). Use
/// `Arc<impl SharingBackend>` when sharing across tasks.
pub trait SharingBackend: JmapBackend {
    /// Create a new object.
    ///
    /// Returns `(assigned_id, created_object)` on success. `create_id` is the
    /// client-side creation id used in the `/set` request.
    ///
    /// # Invariant — MUST hold; not enforced at the type level
    ///
    /// The returned `O` MUST have its `id` field set to the **same**
    /// server-assigned [`Id`](jmap_types::Id) returned as the first element
    /// of the tuple. The handler serializes `O` into the `created` response
    /// map per RFC 8620 §5.3 and DOES NOT cross-check the `id` against the
    /// tuple's `Id` — the client sees the value as serialized from `O`.
    ///
    /// Failing this invariant ships a silent wire-protocol bug: every
    /// `Principal/set create` response carries the wrong `id` (typically a
    /// `"placeholder"` literal injected by the handler — see
    /// `principal.rs` create branch), and no `cargo test` of the backend
    /// alone will catch it. End-to-end JMAP integration is the only signal.
    ///
    /// Reference implementations: see [`memory::MemoryBackend::create_object`]
    /// (`src/memory.rs`) for the recommended shape — serialize to JSON, set
    /// `val["id"]`, deserialize back to `O`. The canonical sibling
    /// `jmap-mail-server`'s `MemoryBackend::create_object` uses the same
    /// pattern.
    ///
    /// Backends that mint the `id` BEFORE constructing `O` (e.g. on a
    /// stored-procedure RETURNING clause, or a column-default trigger) can
    /// build `O` with the canonical id directly and skip the
    /// serialize-mutate-deserialize round-trip.
    ///
    /// Tracking: `bd:JMAP-3t94.17` (this gap was identified during the
    /// `9c0d34f` review pass and may motivate a future workspace-wide
    /// handler-side defensive id-patch — file a workspace-architectural
    /// decision bead before reshaping this trait).
    ///
    /// [`memory::MemoryBackend::create_object`]: crate::memory::MemoryBackend
    fn create_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        create_id: &str,
        obj: O,
    ) -> impl std::future::Future<Output = Result<(jmap_types::Id, O), BackendSetError<Self::Error>>>
           + Send;

    /// Apply a partial update (patch) to an existing Principal.
    ///
    /// Returns `Some(updated_object)` if the backend modified any properties
    /// beyond what the client requested (RFC 8620 §5.3 server-set field echo),
    /// or `None` if the patch was applied verbatim.
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: O::Patch,
    ) -> impl std::future::Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy a Principal or ShareNotification by id.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Returns true if this account supports the given JMAP object type.
    ///
    /// Called by the server consumer (e.g. the session capability builder) —
    /// NOT called internally by the handler library. Backends that support all
    /// types unconditionally can return `true` always.
    ///
    /// Example: a backend backed by a read-only external directory might return
    /// `false` for `Principal` writes, though `forbidden` SetErrors from
    /// `create_object`/`update_object`/`destroy_object` are the primary
    /// enforcement mechanism.
    fn supports_type<O: JmapObject>(&self) -> bool;
}
