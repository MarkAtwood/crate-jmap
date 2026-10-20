//! SceneBackend trait and supporting types for the JMAP Scene method
//! handlers.
//!
//! Read-side operations (`get_objects`, `get_state`, `get_changes`,
//! `query_objects`, `query_changes`) are defined on the
//! [`jmap_server::JmapBackend`] supertrait. This module adds the write
//! operations and the deployment facts the handlers consult.
//!
//! The `pub use` block re-exports the [`jmap_server`] backend types so a
//! downstream impl can `use jmap_scene_server::backend::*`; each re-export
//! pins the upstream type's major-version contract into this crate.

pub use jmap_server::{
    AddedItem, BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject,
    JmapBackend, JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType,
    SetObject,
};

use jmap_scene_types::{SceneAsset, SceneCapability, SceneRegion};

/// What [`SceneBackend::region_binding`] knows about the object a
/// SceneRegion binding property references.
///
/// `SceneRegion/set` maps it to the SetError the draft requires
/// (draft-atwood-jmap-scene-00 §Creating a Region, §Updating a Region).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingTarget {
    /// The object exists and the caller may bind it: for `activeCallId`,
    /// the VTCCall is not `"ended"` and the caller is a participant in or
    /// moderator of it, or an administrator.
    Usable,
    /// The object does not exist; for `activeCallId`, also a VTCCall in
    /// state `"ended"`. `SceneRegion/set` returns `invalidProperties`
    /// naming the property.
    Missing,
    /// The object exists but the caller cannot access it. For `chatId`,
    /// `spaceId` and `channelId` this is `invalidProperties` naming the
    /// property ("if the referenced object does not exist or the caller
    /// cannot access it"); for `activeCallId` it is `forbidden` ("and
    /// `forbidden` if the caller lacks access").
    Inaccessible,
}

/// Storage backend for the JMAP Scene method handlers
/// (draft-atwood-jmap-scene-00).
///
/// # Permission enforcement (backend canonical)
///
/// Per the workspace AGENTS.md "Caller identity (foundation seam)" rule the
/// backend re-verifies every permission atomically with the mutation. The
/// handlers apply the draft's checks (media type, blob, size quota, the
/// owner-or-admin rule via [`Self::may_modify_asset`]) as defense in depth.
///
/// This trait is not object-safe by design (generic methods). Use
/// `Arc<impl SceneBackend>` when sharing across tasks.
pub trait SceneBackend: JmapBackend {
    /// The account-level `urn:ietf:params:jmap:scene` capability object
    /// (draft-atwood-jmap-scene-00 §Account-Level Capability Object) the
    /// consumer advertises for `account_id` to this caller.
    ///
    /// `SceneAsset/set` reads `supportedVisualTypes` and
    /// `maxAssetSizeBytes` from it. Return the value the Session object
    /// carries; a different value makes the handler contradict the
    /// advertised capability. `supportedVisualTypes` "MUST include at least
    /// `\"model/gltf-binary\"`"; the handler accepts that type whether or
    /// not it is listed.
    fn scene_capability(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<SceneCapability, Self::Error>> + Send;

    /// The size in bytes of blob `blob_id`, or `None` if no such blob was
    /// uploaded to `account_id` (RFC 8620 §6.1).
    ///
    /// `SceneAsset/set` create stores the size as the asset's `size` and
    /// refuses a missing blob with `notFound` ("`blobId` does not reference
    /// a valid blob"). There is no default: a default would invent sizes or
    /// refuse every create. Return `Err` for a transient storage failure,
    /// never `Ok(None)`: the handler maps `Err` to `serverFail` so the
    /// client retries.
    fn blob_size(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        blob_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Option<u64>, Self::Error>> + Send;

    /// Create an object.
    ///
    /// The handler has validated `obj` against draft-atwood-jmap-scene-00
    /// §SceneAsset/set and filled the server-set properties; its `id` is a
    /// placeholder. The backend MUST, in one atomic step, assign a ULID
    /// `id` ("A ULID assigned at creation") and store the object.
    ///
    /// For a `SceneAsset` the backend also enforces the deployment-defined
    /// rules of §SetError Conditions atomically with the insert: refuse
    /// with `overQuota` for "Account asset storage quota
    /// exceeded", and with `forbidden` when the "Deployment restricts
    /// shared-asset creation to administrators" and the caller is not one.
    ///
    /// For a `SceneRegion` the backend refuses with `overQuota` when the
    /// account already holds `maxRegionsPerAccount` regions ("The server
    /// MUST return `overQuota` when `maxRegionsPerAccount` would be
    /// exceeded"), counted atomically with the insert.
    ///
    /// Returns the assigned id and the stored object. `create_id` is the
    /// client's creation id. Error mapping, state-counter and change-log
    /// rules are those of the canonical `jmap-mail-server` `create_object`:
    /// the state advances only when the create commits.
    fn create_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        create_id: &str,
        obj: O,
    ) -> impl std::future::Future<Output = Result<(jmap_types::Id, O), BackendSetError<Self::Error>>>
           + Send;

    /// Apply an RFC 8620 §5.3 PatchObject to an existing object.
    ///
    /// `Ok(Some(o))` echoes server-changed properties; `Ok(None)` means the
    /// patch applied verbatim. Both MUST advance the state and record an
    /// `updated` change. Unknown `id` is `notFound`; a malformed patch is
    /// `invalidPatch`; a result that no longer decodes is
    /// `invalidProperties`. Apply the whole patch or none of it.
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: O::Patch,
    ) -> impl std::future::Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy an object by id. Unknown `id` is `notFound`; the state
    /// advances only when the destroy commits. The handler has already
    /// applied the permission check.
    ///
    /// Destroying a `SceneRegion` "removes the SceneRegion and all
    /// contained SceneObject and SceneAvatar records" (draft-atwood-jmap-
    /// scene-00 §Destroying a Region): in the same atomic step the backend
    /// destroys every SceneObject and SceneAvatar whose `regionId` is `id`
    /// and advances those types' states. It does not end a VTCCall the
    /// region's `activeCallId` references ("the server MUST NOT
    /// automatically end the referenced VTCCall").
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// `true` if the caller may update or destroy `asset`: "Only the asset
    /// owner or an admin may modify asset metadata"
    /// (draft-atwood-jmap-scene-00 §SceneAsset/set Update Errors; Destroy
    /// Errors use the "Same permission model as update").
    ///
    /// `SceneAsset/set` returns `forbidden` for an update or destroy when
    /// this is `false`.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(true)`: every caller may modify every asset. This is the
    /// workspace's fail-open posture for permission hooks (bd:JMAP-bghk);
    /// production backends override it.
    fn may_modify_asset(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        asset: &SceneAsset,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        let _ = (caller, account_id, asset);
        async { Ok(true) }
    }

    /// `true` if the consumer advertises the capability `uri` for
    /// `account_id`.
    ///
    /// The SceneRegion binding properties "are present only when the
    /// corresponding companion capability is advertised on the same
    /// account": `chatId`, `spaceId` and `channelId` need
    /// `urn:ietf:params:jmap:chat`, `activeCallId` needs
    /// `urn:ietf:params:jmap:vtc`. `SceneRegion/set` refuses a non-null
    /// binding without its capability, and `accessPolicy` `"space"`
    /// without `urn:ietf:params:jmap:chat`, with `invalidProperties`; a
    /// create stores `null` for the bindings whose capability is
    /// advertised.
    fn has_capability(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        uri: &str,
    ) -> bool;

    /// What `property` (`"chatId"`, `"spaceId"`, `"channelId"` or
    /// `"activeCallId"`) set to `id` references, for the checks of
    /// draft-atwood-jmap-scene-00 §Creating a Region ("the server MUST
    /// verify that the referenced object exists and the caller has access
    /// to it") and §Updating a Region (for `activeCallId`: "(a) the
    /// referenced VTCCall exists, (b) its state is not `\"ended\"`, and (c)
    /// the caller is a participant in or moderator of the referenced call,
    /// or has deployment-defined administrative privileges").
    ///
    /// `SceneRegion/set` calls this for each non-null binding a create
    /// sets or an update patches, after the capability check. The Chat and
    /// VTC lookups live in the binding crates; this crate depends on
    /// RFC 8620 only.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(BindingTarget::Usable)`: every reference is accepted.
    /// This is the workspace's fail-open posture for permission hooks
    /// (bd:JMAP-bghk); a backend that advertises Chat or VTC overrides it.
    fn region_binding(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        property: &str,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<BindingTarget, Self::Error>> + Send {
        let _ = (caller, account_id, property, id);
        async { Ok(BindingTarget::Usable) }
    }

    /// `true` if the caller may update `region`: "Only the owner or an
    /// admin may modify region properties" (draft-atwood-jmap-scene-00
    /// §SceneRegion/set Update Errors).
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(true)` (fail-open, bd:JMAP-bghk); production backends
    /// override it.
    fn may_update_region(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        region: &SceneRegion,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        let _ = (caller, account_id, region);
        async { Ok(true) }
    }

    /// `true` if the caller may destroy `region`: "Only the region owner
    /// may destroy a region" (draft-atwood-jmap-scene-00 §SceneRegion/set
    /// Destroy Errors). Unlike update, administrators are not named.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(true)` (fail-open, bd:JMAP-bghk); production backends
    /// override it.
    fn may_destroy_region(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        region: &SceneRegion,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        let _ = (caller, account_id, region);
        async { Ok(true) }
    }

    /// Per-backend capability probe: `true` if this backend supports the
    /// JMAP object type `O` for `account_id`. Not called by the handlers;
    /// consumers use it when assembling their Session capabilities
    /// ("Deployments ... MAY omit support for SceneAsset methods").
    fn supports_type<O: JmapObject>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> bool;
}
