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
///
/// # Caller identity (foundation seam)
///
/// Per the workspace AGENTS.md "Caller identity (foundation seam)" rule,
/// this trait reads caller identity exclusively via
/// [`JmapBackend::principal_id`] on the supertrait. The returned
/// `Option<&jmap_types::Id>` is the canonical input to every per-object
/// permission decision made inside `create_object`, `update_object`, and
/// `destroy_object` — there is no alternate path, no
/// `caller_identity_blob()` escape hatch, and no generic claims map.
///
/// **Production deployments MUST override `principal_id`.** The default
/// supertrait impl returns `None`, which signals "this deployment does
/// not honor identity-dependent JMAP semantics". A backend that leaves
/// the default in place CANNOT correctly implement RFC 9670 `myRights`
/// semantics: every Principal / ShareNotification read SHOULD be
/// authorized against the caller's effective rights on the target
/// principal, and a `None` caller removes the authorization input.
///
/// **Single-user dev backends and test fixtures** may leave the default
/// `None` impl in place. The reference [`memory::MemoryBackend`] does
/// this and is correct for its single-user, in-memory, demonstration-
/// only use case.
///
/// This crate is the canonical source of truth for the `myRights`
/// field that other extension-server crates (mail, calendars, tasks,
/// contacts, filenode) propagate to their own shareable objects.
/// The contract above is therefore workspace-load-bearing, not a
/// sharing-server-internal concern.
///
/// # Permission enforcement (backend canonical)
///
/// Per the workspace AGENTS.md "Permission enforcement: backend
/// canonical" rule, handlers do NO permission checking. Defense-in-
/// depth handler-side pre-checks are allowed but the backend MUST
/// re-verify atomically with the mutation. A handler that "trusts" a
/// handler-side check and skips the backend re-check is a bug. The
/// per-method docs on `create_object`, `update_object`, and
/// `destroy_object` restate this contract; the foundation rule lives
/// here.
///
/// [`memory::MemoryBackend`]: crate::memory::MemoryBackend
pub trait SharingBackend: JmapBackend {
    /// Create a new object.
    ///
    /// Returns `(assigned_id, created_object)` on success.
    ///
    /// # Error contract (RFC 8620 §5.3)
    ///
    /// - **Caller lacks permission to create on `account_id`**: return
    ///   `BackendSetError::SetError(SetError::new(SetErrorType::Forbidden))`.
    ///   Per the workspace AGENTS.md "Permission enforcement: backend
    ///   canonical" rule, this trait method is the canonical point of
    ///   enforcement for create-time permission checks. Handlers do NOT
    ///   re-verify permission; a backend that omits the check ships a
    ///   security bug.
    /// - **`account_id` unknown**: the handler at
    ///   `principal.rs::handle_principal_set` /
    ///   `notification.rs::handle_share_notification_set` calls
    ///   [`JmapBackend::account_exists`] before reaching this method
    ///   and returns `accountNotFound` at the top level on a `false`
    ///   result. Backend implementors MAY treat `account_id` as
    ///   "verified to exist" when invoked through the standard
    ///   handler path, but defense-in-depth (returning `Forbidden`
    ///   or surfacing a typed storage error for an unknown account)
    ///   is RECOMMENDED.
    /// - **Submitted property values violate the object's schema**:
    ///   return `BackendSetError::SetError(SetError::new(
    ///   SetErrorType::InvalidProperties))`, optionally populated
    ///   with the offending property names via
    ///   [`SetError::with_properties`](jmap_server::SetError).
    ///   The handler's deserialize-into-`O` pre-check at
    ///   `principal.rs::handle_principal_set` (`invalidProperties`
    ///   wire mapping) catches malformed JSON shapes; the backend is
    ///   responsible for the semantic-validation tier (uniqueness,
    ///   cross-field consistency, FK references).
    /// - **Per-account or per-system quota exceeded**: return
    ///   `BackendSetError::SetError(SetError::new(SetErrorType::OverQuota))`.
    /// - **Singleton-class object already exists** (RFC 8620 §5.3
    ///   `singleton`): return
    ///   `BackendSetError::SetError(SetError::new(SetErrorType::Singleton))`.
    ///   None of the present RFC 9670 object types are singletons,
    ///   but the contract admits the variant for future-spec
    ///   compatibility.
    /// - **Other storage failure**: return
    ///   `BackendSetError::Other(your_error_type)`; the handler maps
    ///   this to a `serverFail` SetError on the wire.
    ///
    /// # State-mutation contract
    ///
    /// - The state counter (`get_state::<O>`) MUST advance only if the
    ///   create commits. A failure path (Forbidden, InvalidProperties,
    ///   OverQuota, storage error) MUST leave the state unchanged.
    /// - The change log (`get_changes::<O>`) MUST record a `created`
    ///   entry for the assigned id atomically with the state advance.
    ///   A reader calling `/changes` after a successful create MUST
    ///   see the id in the `created` array.
    /// - On the round-trip-deserialize failure path (see below), the
    ///   backend MUST NOT have committed any of the above.
    ///
    /// # Atomicity ordering — deserialize before commit
    ///
    /// Backends that use the recommended serialize-mutate-deserialize
    /// round-trip to enforce the id invariant (see "Invariant" below)
    /// MUST perform the deserialize step BEFORE committing any
    /// storage mutation, state-counter advance, or change-log entry.
    /// A backend that commits storage first and deserializes after
    /// leaks a silent state advance whenever the deserialize fails:
    /// the on-wire state counter moves but no observable object
    /// appears, and `/changes` reports a `created` id that
    /// `/get` cannot resolve.
    ///
    /// The reference [`memory::MemoryBackend::create_object`]
    /// (`src/memory.rs`) demonstrates the correct ordering: serialize,
    /// patch `val["id"]`, deserialize into `O`, THEN advance state,
    /// THEN insert into the objects map, THEN push the change-log
    /// entry. Any earlier failure surfaces as
    /// `BackendSetError::Other(...)` without side effects.
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

    /// Apply a partial update (patch) to an existing object of type `O`.
    ///
    /// `O` is generic over [`SetObject`]; for this crate that is
    /// [`Principal`](jmap_sharing_types::Principal) or
    /// [`ShareNotification`](jmap_sharing_types::ShareNotification).
    /// The current handler at `notification.rs::handle_share_notification_set`
    /// short-circuits `ShareNotification` updates with `forbidden` per
    /// RFC 9670 §3.3 before reaching the backend, but the trait shape
    /// remains generic.
    ///
    /// # Return value
    ///
    /// - `Ok(Some(updated_object))`: the backend modified properties
    ///   beyond what the client requested (RFC 8620 §5.3 server-set
    ///   field echo). The handler serializes the returned `O` into
    ///   the `updated` map.
    /// - `Ok(None)`: the patch applied verbatim. The handler stores
    ///   `null` for the id in the `updated` map per RFC 8620 §5.3.
    ///
    /// # Error contract (RFC 8620 §5.3)
    ///
    /// - **`id` not found in `account_id`**: return
    ///   `BackendSetError::SetError(SetError::new(SetErrorType::NotFound))`.
    ///   The handler maps this verbatim into `notUpdated[id].type`.
    /// - **Caller lacks permission to update `id`**: return
    ///   `BackendSetError::SetError(SetError::new(SetErrorType::Forbidden))`.
    ///   Per the workspace AGENTS.md "Permission enforcement: backend
    ///   canonical" rule, this trait method is the canonical point of
    ///   enforcement for per-object update permission. Handlers do NOT
    ///   re-verify permission; a backend that omits the check ships a
    ///   security bug.
    /// - **`account_id` unknown**: the handler at
    ///   `principal.rs::handle_principal_set` /
    ///   `notification.rs::handle_share_notification_set` calls
    ///   [`JmapBackend::account_exists`] before reaching this method
    ///   and returns `accountNotFound` at the top level on a `false`
    ///   result. Backend implementors MAY treat `account_id` as
    ///   "verified to exist" when invoked through the standard
    ///   handler path, but defense-in-depth (returning `NotFound`
    ///   for an unknown account, as the reference
    ///   [`memory::MemoryBackend`] does) is RECOMMENDED.
    /// - **Patch shape is malformed** (e.g. JSON Pointer references
    ///   a non-existent path, or a leaf assignment violates the
    ///   target field's type): return
    ///   `BackendSetError::SetError(SetError::new(
    ///   SetErrorType::InvalidPatch))`. The handler's
    ///   `PatchObject` deserialize pre-check catches structurally
    ///   invalid JSON; the backend is responsible for the semantic
    ///   tier (referenced paths exist, value types match).
    /// - **Patched values violate the object's schema**: return
    ///   `BackendSetError::SetError(SetError::new(
    ///   SetErrorType::InvalidProperties))`, optionally populated
    ///   with the offending property names via
    ///   [`SetError::with_properties`](jmap_server::SetError).
    /// - **Per-account or per-system quota exceeded by the patch**:
    ///   return
    ///   `BackendSetError::SetError(SetError::new(SetErrorType::OverQuota))`.
    /// - **`id` is scheduled for destroy in the same `/set` call**
    ///   (RFC 8620 §5.3 `willDestroy`): return
    ///   `BackendSetError::SetError(SetError::new(SetErrorType::WillDestroy))`.
    ///   The handler does NOT reorder operations; backends that
    ///   detect this race may surface it here.
    /// - **Other storage failure**: return
    ///   `BackendSetError::Other(your_error_type)`; the handler maps
    ///   this to a `serverFail` SetError on the wire.
    ///
    /// # State-mutation contract
    ///
    /// - The state counter (`get_state::<O>`) MUST advance only if
    ///   the update commits. A failure path (NotFound, Forbidden,
    ///   InvalidPatch, InvalidProperties, OverQuota, storage error)
    ///   MUST leave the state unchanged.
    /// - The change log (`get_changes::<O>`) MUST record an
    ///   `updated` entry for the id atomically with the state
    ///   advance. A reader calling `/changes` after a successful
    ///   update MUST see the id in the `updated` array.
    /// - When the return value is `Ok(None)` (patch applied
    ///   verbatim, no server-side property echo), the state counter
    ///   and change log MUST still advance — `Ok(None)` is a
    ///   wire-shape signal to the handler, NOT a "nothing
    ///   happened" signal.
    ///
    /// # Idempotency
    ///
    /// Repeated identical patches against the same id produce a new
    /// state advance and a new change-log `updated` entry each
    /// time. RFC 8620 §5.3 does not require idempotency; this
    /// matches the workspace convention shared with the canonical
    /// `jmap-mail-server`.
    ///
    /// [`memory::MemoryBackend`]: crate::memory::MemoryBackend
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
