//! SharingBackend trait and supporting types for JMAP Sharing method handlers.
//!
//! Consumers implement [`SharingBackend`] for your storage system. The method
//! handlers in sibling modules call into the backend through this trait.
//!
//! The read-side operations (`get_objects`, `get_state`, `get_changes`,
//! `query_objects`, `query_changes`) are defined on the [`jmap_server::JmapBackend`]
//! supertrait. Only write operations are here.
//!
//! Marker traits and property selector enums live in `jmap-types` and
//! `jmap-sharing-types` respectively; they are re-exported here for convenience.
//!
//! # About the re-exports
//!
//! This module re-exports a bundle of types from [`jmap_server`] and
//! [`jmap_sharing_types::backend`] so a downstream `SharingBackend` impl
//! can `use jmap_sharing_server::backend::*` without separately importing
//! the foundation crate and the types crate. The re-exported items are
//! GROUPED here, not redefined — their canonical home is the source crate.
//!
//! **Version pinning**: each `pub use` pins the upstream type's major-
//! version contract into this crate's public surface. A SemVer-breaking
//! change in [`jmap_server`] or [`jmap_sharing_types`] cascades into a
//! SemVer-breaking change here. The workspace's path-dep coordination
//! and synchronized minor-version cadence (`0.1.x` across all `jmap-*`
//! crates) is what keeps this manageable.
//!
//! **Discoverability**: callers importing via the re-export should
//! consult the upstream rustdoc for canonical documentation:
//! - [`AddedItem`], [`BackendChangesError`], [`BackendSetError`],
//!   [`ChangesResult`], [`GetObject`], [`JmapBackend`], [`JmapObject`],
//!   [`QueryChangesResult`], [`QueryObject`], [`QueryResult`],
//!   [`SetError`], [`SetErrorType`], [`SetObject`] → [`jmap_server`]
//! - [`PrincipalProperty`], [`ShareNotificationProperty`] →
//!   [`jmap_sharing_types::backend`]
//!
//! [`PrincipalProperty`]: jmap_sharing_types::backend::PrincipalProperty
//! [`ShareNotificationProperty`]: jmap_sharing_types::backend::ShareNotificationProperty

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
    /// Returns `(assigned_id, created_object)` on success.
    ///
    /// # `create_id` parameter — passthrough only
    ///
    /// `create_id` is the client-side creation id from the `/set` request's
    /// `create` map key (e.g. `"c1"` in `{"create": {"c1": {...}}}`).
    /// Backends typically **ignore this argument**; both in-tree reference
    /// impls ([`memory::MemoryBackend`] and the test `MockBackend`) bind
    /// it as `_create_id`. It is plumbed through the trait for two
    /// permitted backend uses:
    ///
    /// 1. **Audit / diagnostic logging** — record which client-side
    ///    creation id maps to which server-assigned [`jmap_types::Id`].
    /// 2. **Idempotent retry detection** — a backend that wants to be
    ///    robust against client retries within a single TCP connection
    ///    may use `create_id` plus the caller identity as a
    ///    deduplication key. This is OPTIONAL; the handler does NOT
    ///    enforce idempotency.
    ///
    /// `create_id` is NOT used for `#cid` ResultReference resolution
    /// (RFC 8620 §3.7). That resolution operates on the `/set` response's
    /// `created` map (keyed by `create_id`) inside the
    /// [`jmap_server::Dispatcher`], not on the backend's input. Backends
    /// have no need to participate in `#cid` resolution.
    ///
    /// [`memory::MemoryBackend`]: crate::memory::MemoryBackend
    /// [`jmap_server::Dispatcher`]: jmap_server::Dispatcher
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
    ///
    /// # Error contract (RFC 8620 §5.3)
    ///
    /// - **`id` not found**: return
    ///   `BackendSetError::SetError(SetError::new(SetErrorType::NotFound))`.
    ///   The handler maps this verbatim into the `notDestroyed[id].type`
    ///   field per RFC 8620 §5.3 `notDestroyed`.
    /// - **Caller lacks permission to destroy `id`**: return
    ///   `BackendSetError::SetError(SetError::new(SetErrorType::Forbidden))`.
    ///   Per the workspace AGENTS.md "Permission enforcement: backend
    ///   canonical" rule, this trait method is the canonical point of
    ///   enforcement for per-object permission checks. Handlers do NOT
    ///   re-verify permission; a backend that omits the check ships a
    ///   security bug.
    /// - **`account_id` unknown**: the handler at
    ///   `notification.rs::handle_share_notification_set` /
    ///   `principal.rs::handle_principal_set` calls
    ///   [`JmapBackend::account_exists`] before reaching this method
    ///   and returns `accountNotFound` at the top level on a `false`
    ///   result. Backend implementors MAY treat this method's
    ///   `account_id` argument as "verified to exist" when invoked
    ///   through the standard handler path, but defense-in-depth
    ///   (returning `NotFound` for an unknown account, as the
    ///   reference [`memory::MemoryBackend`] does) is RECOMMENDED.
    /// - **Other storage failure**: return
    ///   `BackendSetError::Other(your_error_type)`; the handler maps
    ///   this to a `serverFail` SetError on the wire.
    ///
    /// # State-mutation contract
    ///
    /// - The state counter (`get_state::<O>`) MUST advance only if the
    ///   destroy commits. A failure path (NotFound, Forbidden, storage
    ///   error) MUST leave the state unchanged.
    /// - The change log (`get_changes::<O>`) MUST record a `destroyed`
    ///   entry for the id atomically with the state advance. A reader
    ///   calling `/changes` after a successful destroy MUST see the id
    ///   in the `destroyed` array.
    ///
    /// # Idempotency
    ///
    /// Destroying an already-destroyed id is `NotFound`. RFC 8620 §5.3
    /// does not require either shape; this is the workspace convention
    /// shared with the canonical `jmap-mail-server` and aligns with
    /// "second destroy is a request to destroy something that does not
    /// exist".
    ///
    /// [`memory::MemoryBackend`]: crate::memory::MemoryBackend
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Per-**backend** capability probe: `true` if this backend supports
    /// the given JMAP object type `O` at all.
    ///
    /// This is NOT called internally by the handler library — the handler
    /// always forwards `create_object` / `update_object` / `destroy_object`
    /// to the backend regardless of this return value, and per-object
    /// authorization is the backend's own responsibility (returning a
    /// `forbidden` [`SetError`] is the wire-visible enforcement
    /// mechanism). Backends that support all types unconditionally
    /// SHOULD return `true` always; both in-tree reference impls
    /// ([`memory::MemoryBackend`] and the test `MockBackend`) do exactly
    /// this.
    ///
    /// # Intended consumer use
    ///
    /// The hook exists so a downstream consumer assembling a JMAP Session
    /// capability response (per RFC 8620 §2 / §3.1) can hide methods the
    /// backend categorically does not support. For example, a backend
    /// backed by a read-only external directory might return `false` for
    /// `Principal` so the consumer's session-capability builder omits
    /// `Principal/*` from the advertised capabilities. The workspace does
    /// not ship a session-capability builder; that is the consumer's
    /// responsibility (see workspace AGENTS.md: "transport/multi-tenancy/
    /// auth/storage is the consumer's responsibility").
    ///
    /// # Per-backend, NOT per-account
    ///
    /// The signature takes no `account_id`: a `false` return means the
    /// backend cannot host this object type for ANY account. Per-account
    /// restrictions belong on the SetError path
    /// (`forbidden` / `accountNotFound`), not here.
    ///
    /// [`memory::MemoryBackend`]: crate::memory::MemoryBackend
    fn supports_type<O: JmapObject>(&self) -> bool;
}
