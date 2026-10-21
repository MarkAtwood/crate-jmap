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

use jmap_scene_types::{
    SceneAsset, SceneAvatar, SceneCapability, SceneInteractionEvent, SceneObject, SceneRegion,
};

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
    /// Every `SceneAsset/*` handler reads `supportsSceneAsset` from it and
    /// returns `unknownMethod` when it is `false`; `SceneAsset/set` also
    /// reads `supportedVisualTypes` and `maxAssetSizeBytes`. Return the value the Session object
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
    /// refuses a missing blob with `invalidProperties` naming `blobId`
    /// ("`blobId` does not reference a valid blob"). There is no default: a default would invent sizes or
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
    /// For a `SceneObject` the backend sets `ownerId` to the caller's
    /// userId ("The userId of the user who created this object"; the
    /// handler sends `null`) and refuses with `overQuota` when the object's
    /// region already holds `maxObjectsPerRegion` objects ("The server MUST
    /// return `overQuota` when `maxObjectsPerRegion` would be exceeded"),
    /// counted atomically with the insert.
    ///
    /// For a `SceneAvatar` (§Entering a Region) the handler has filled
    /// `userId` from [`Self::caller_user_id`], `position`, `orientation`,
    /// `joinedAt` (now) and `leftAt` (`null`). In one atomic step the
    /// backend:
    ///
    /// 1. refuses with `alreadyExists` and `existingId` naming the record
    ///    when the user already has an avatar in the region, present or
    ///    departed ("the server MUST NOT create a second record and MUST
    ///    return an `alreadyExists` SetError ... with `existingId` set to
    ///    the id of that record");
    /// 2. refuses with `overQuota` when the region already holds
    ///    `maxAvatarsPerRegion` avatars with `leftAt` `null`;
    /// 3. sets `leftAt` to the new `joinedAt` on the user's active avatar
    ///    in any other region ("the server SHOULD set `leftAt` on the
    ///    previous avatar before creating the new one");
    /// 4. keeps each affected region's `activeAvatarCount` ("Number of
    ///    avatars currently present in the region") equal to its avatars
    ///    with `leftAt` `null`, advancing the SceneRegion state.
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
    ///
    /// For a `SceneAvatar` whose patch sets `leftAt` to a UTCDate, the
    /// backend refuses with `invalidProperties` naming `leftAt` when the
    /// stored `leftAt` is already non-null ("double-leave is invalid"), and
    /// otherwise decrements the region's `activeAvatarCount` in the same
    /// atomic step ("The server decrements the region's
    /// `activeAvatarCount`"), advancing the SceneRegion state.
    ///
    /// A patch that sets `leftAt` to `null` re-enters the region
    /// (§Re-entering a Region); the handler has added `joinedAt` (now),
    /// `position` and `orientation` (the region's spawn point). In the
    /// same atomic step the backend refuses with `invalidProperties`
    /// naming `leftAt` when the stored `leftAt` is `null`, then applies
    /// steps 2 to 4 of [`Self::create_object`] for a `SceneAvatar`: the
    /// `maxAvatarsPerRegion` `overQuota`, setting `leftAt` on the user's
    /// active avatar in any other region, and the `activeAvatarCount`
    /// update.
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
    /// SceneObject and SceneAvatar records in it; `SceneObject/changes` and
    /// `SceneAvatar/changes` report those records as destroyed"
    /// (draft-atwood-jmap-scene-00 §Destroying a Region): in the same atomic
    /// step the backend destroys every SceneObject and SceneAvatar whose
    /// `regionId` is `id` and records each as a destroyed change of its
    /// type. It does not end a VTCCall the
    /// region's `activeCallId` references ("the server MUST NOT
    /// automatically end the referenced VTCCall").
    ///
    /// Destroying a `SceneObject` "removes the SceneObject and all its
    /// children (objects whose `parentId` references this object,
    /// recursively)": in the same atomic step the backend destroys every
    /// descendant and records each as a destroyed SceneObject change.
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

    /// `true` if `channel_id` "is a channel of the Space named by
    /// `spaceId`" (draft-atwood-jmap-scene-00 §Updating a Region, the
    /// binding checks).
    ///
    /// `SceneRegion/set` calls this when a create or an update sets
    /// `channelId` or `spaceId` and the region as it would be after the
    /// write has both non-null, after [`Self::region_binding`] accepted
    /// each. `false` fails the write with `invalidProperties` naming
    /// `channelId`.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(true)` (fail-open, bd:JMAP-bghk); a backend that
    /// advertises Chat overrides it.
    fn channel_in_space(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        channel_id: &jmap_types::Id,
        space_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        let _ = (caller, account_id, channel_id, space_id);
        async { Ok(true) }
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
    /// or an admin may destroy a region" (draft-atwood-jmap-scene-00
    /// §SceneRegion/set Destroy Errors).
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

    /// `true` if the caller may update or destroy `object`: "Only the
    /// object's `ownerId`, the region owner, or a deployment-admin may
    /// modify another user's object" (draft-atwood-jmap-scene-00
    /// §SceneObject/set Update Errors; Destroy Errors use the "Same
    /// permission model as update").
    ///
    /// `SceneObject/set` returns `forbidden` for an update or destroy when
    /// this is `false`.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(true)` (fail-open, bd:JMAP-bghk); production backends
    /// override it.
    fn may_modify_object(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        object: &SceneObject,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        let _ = (caller, account_id, object);
        async { Ok(true) }
    }

    /// The caller's userId: "The authenticated user identity string"
    /// (draft-atwood-jmap-scene-00 §SceneAvatar `userId`).
    ///
    /// `SceneAvatar/set` stores it as a created avatar's `userId` ("The
    /// server sets `userId` from the authenticated identity") and compares
    /// it with an updated avatar's `userId` ("Users MUST NOT update other
    /// users' avatars"). There is no default: the handlers cannot know the
    /// identity.
    fn caller_user_id(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<String, Self::Error>> + Send;

    /// The `displayName` of a new avatar for `user_id` when the create
    /// supplies none: "the server sets it from the user profile or
    /// ChatContact when {{JMAP-CHAT}} is present".
    ///
    /// # Default implementation
    ///
    /// Returns `user_id`. A Chat binding crate or a backend with user
    /// profiles overrides it.
    fn avatar_display_name(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        user_id: &str,
    ) -> impl std::future::Future<Output = Result<String, Self::Error>> + Send {
        let _ = (caller, account_id);
        let name = user_id.to_owned();
        async { Ok(name) }
    }

    /// The reserved display names (draft-atwood-jmap-scene-00 §Avatar
    /// Identity and Impersonation): "The server SHOULD maintain a list of
    /// reserved display names that correspond to system roles or
    /// administrative functions and MUST reject `SceneAvatar/set`
    /// operations that attempt to use a reserved name with a `SetError` of
    /// type `forbidden`."
    ///
    /// `SceneAvatar/set` refuses a create or update whose client-supplied
    /// `displayName` equals one of these names exactly with `forbidden`.
    /// The comparison is case-sensitive, so list each spelling to reserve.
    /// A name set by [`Self::avatar_display_name`] is not checked.
    ///
    /// # Default implementation
    ///
    /// Returns no names (fail-open, bd:JMAP-bghk); deployments supply
    /// their list.
    fn reserved_display_names(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Vec<String>, Self::Error>> + Send {
        let _ = (caller, account_id);
        async { Ok(Vec::new()) }
    }

    /// `true` unless the caller is barred from entering `region`, which
    /// it already has access to: "User is banned or ejected with a
    /// cooldown period still active" (draft-atwood-jmap-scene-00
    /// §Create Errors). `SceneAvatar/set` create and an update that
    /// re-enters a region return `forbidden` when this is `false`, since
    /// "The user already knows the region exists". Access under the
    /// region's `accessPolicy` is [`Self::may_access_region`], not this
    /// hook: a create without it is `invalidProperties` naming
    /// `regionId`, a re-entry `notFound`.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(true)` (fail-open, bd:JMAP-bghk); production backends
    /// override it.
    fn may_enter_region(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        region: &SceneRegion,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        let _ = (caller, account_id, region);
        async { Ok(true) }
    }

    /// `true` if the caller may eject `avatar`, another user's avatar:
    /// "region owners and administrators may set `leftAt` on another
    /// user's avatar to eject them" (draft-atwood-jmap-scene-00 §Update
    /// Errors).
    ///
    /// `SceneAvatar/set` calls this only for a patch of another user's
    /// avatar that sets `leftAt` and touches nothing else ("An eject
    /// update patches only `leftAt`", §Avatar Permissions); `false` is
    /// `forbidden`.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(true)` (fail-open, bd:JMAP-bghk); production backends
    /// override it.
    fn may_eject_avatar(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        avatar: &SceneAvatar,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        let _ = (caller, account_id, avatar);
        async { Ok(true) }
    }

    /// `true` if the caller has access to region `region_id`
    /// (draft-atwood-jmap-scene-00 §Region Access): "A user has access to a
    /// region when its `accessPolicy` admits the user, when the user is the
    /// region owner, when the user has deployment-defined administrative
    /// privileges, or when the user has an active SceneAvatar in it."
    /// `"invite"` admits "The users whose userIds are listed in
    /// `invitedUserIds`", and "Servers MAY admit users to an `\"invite\"`
    /// region by other mechanisms as well".
    ///
    /// `SceneRegion/get`, `SceneObject/get` and `SceneAvatar/get` report a
    /// region, object or avatar in a region without access as `notFound`:
    /// "Regardless of `accessPolicy`, the server MUST return `notFound`
    /// (not `forbidden`) ... This prevents access-policy enumeration."
    /// `SceneRegion/set`, `SceneObject/set` and `SceneAvatar/set` report an
    /// update or destroy of such a record as `notFound` too, never
    /// `forbidden`. A region that no longer exists
    /// may be treated either way.
    ///
    /// The handlers cannot filter a `/query` page they did not cut, so
    /// `query_objects` and `query_changes` MUST omit what `/get` would
    /// hide: the server "MUST NOT include a SceneObject in a ...
    /// `SceneObject/query` result if the authenticated user does not have
    /// access to the object's containing SceneRegion" (§Visibility
    /// Contract). The same applies to SceneRegion and SceneAvatar results.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(true)` (fail-open, bd:JMAP-bghk); production backends
    /// override it.
    fn may_access_region(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        region_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        let _ = (caller, account_id, region_id);
        async { Ok(true) }
    }

    /// `true` if `object` is in the caller's visibility set: "The server
    /// SHOULD apply visibility filtering to limit the objects returned
    /// based on the authenticated user's avatar position within the
    /// region. The specific visibility algorithm is deployment-defined"
    /// (draft-atwood-jmap-scene-00 §Visibility Contract).
    ///
    /// Consulted only for objects in a region the caller may access
    /// ([`Self::may_access_region`]). `SceneObject/get` reports an object
    /// outside the set as `notFound`, and `SceneObject/set` refuses an
    /// update or destroy of it with `notFound`: "The server MUST NOT return
    /// `forbidden`, as that would confirm the object still exists."
    /// `query_objects` and `query_changes` MUST omit such objects as well.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(true)`: "A simple deployment MAY return all objects in
    /// the region."
    fn is_object_visible(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        object: &SceneObject,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        let _ = (caller, account_id, object);
        async { Ok(true) }
    }

    /// `true` if the caller has blocked `user_id` (draft-atwood-jmap-scene-00
    /// §Blocked-Contact Spatial Presence): "SceneAvatar records belonging
    /// to a blocked ChatContact SHOULD be excluded from `SceneAvatar/get`
    /// responses, `SceneAvatar/query` results, and `SceneAvatar/queryChanges`
    /// notifications delivered to the blocking user."
    ///
    /// Consulted only for avatars in a region the caller may access:
    /// "Blocked-contact filtering is applied AFTER the visibility
    /// filtering". `SceneAvatar/get` reports such an avatar exactly as a
    /// missing one, in `notFound` ("The server MUST NOT return an error, a
    /// filtered-result indicator, or any other signal"); `query_objects`
    /// and `query_changes` MUST omit it as well. The handlers leave
    /// `activeAvatarCount` the true count, which deployments "SHOULD
    /// prefer".
    ///
    /// The Chat lookup (`ChatContact.blocked`) lives in the binding crate:
    /// "Without `urn:ietf:params:jmap:chat`, there is no blocked-contact
    /// list and this filtering does not apply."
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(false)`: no one is blocked. This is the workspace's
    /// fail-open posture for policy hooks (bd:JMAP-bghk).
    fn is_user_blocked(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        user_id: &str,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        let _ = (caller, account_id, user_id);
        async { Ok(false) }
    }

    /// `true` if the caller may see the SceneAvatar records of region
    /// `region_id` (draft-atwood-jmap-scene-00 at 6bcd9d8, L2654): "When a
    /// SceneRegion's `activeCallId` references a VTCCall with
    /// `e2eeEnabled` set to `true`, the server SHOULD NOT expose the
    /// region's SceneAvatar records to users who are not participants in
    /// the bound VTCCall: for such users it SHOULD return `notFound` for
    /// `SceneAvatar/get`, exclude the records from `SceneAvatar/query`, and
    /// suppress `SceneAvatarEvent` ... delivery (`entered`, `left`, and
    /// `ejected`)."
    ///
    /// Consulted only for avatars in a region the caller may access
    /// ([`Self::may_access_region`]). `SceneAvatar/get` reports the avatars
    /// of a region answering `false` in `notFound`, and
    /// [`crate::should_deliver`] drops their `SceneAvatarEvent`s;
    /// `query_objects` and `query_changes` MUST omit them as well.
    ///
    /// The VTC lookup (`VTCCall.e2eeEnabled` and the call's participants)
    /// lives in the binding crate, `jmap-scene-vtc-server`.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(true)`: no region hides its avatars. This is the
    /// workspace's fail-open posture for policy hooks (bd:JMAP-bghk); a
    /// deployment without it exposes the call's participant presence, the
    /// limitation the draft's SHOULD leaves to it.
    fn may_see_region_avatars(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        region_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        let _ = (caller, account_id, region_id);
        async { Ok(true) }
    }

    /// Accept an interaction that passed the `SceneObject/interact`
    /// checks, or refuse it for its rate (draft-atwood-jmap-scene-00
    /// §SceneObject/interact).
    ///
    /// `event` carries the interaction and the caller's userId from
    /// [`Self::caller_user_id`]. `Ok(false)` means "The caller has exceeded
    /// a deployment-defined interaction rate" and the handler reports
    /// `rateLimit`; draft-atwood-jmap-scene-wss-00 §Rate Limiting suggests
    /// "no more than five events per user per region per second".
    /// `Ok(true)` accepts it: count it against the rate and, when the
    /// WebSocket capability is supported, hand `event` to the transport,
    /// which "delivers a `SceneInteractionEvent` carrying the interaction
    /// and the caller's userId to the WebSocket subscribers of the region",
    /// filtered per recipient by [`crate::should_deliver`]. Counting and
    /// delivering in this one call keeps them atomic.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(true)` and delivers nothing: no rate limit (fail-open,
    /// bd:JMAP-bghk) and no WebSocket transport.
    fn accept_interaction(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        event: &SceneInteractionEvent,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        let _ = (caller, account_id, event);
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
