//! SceneBackend trait and supporting types for the JMAP Scene method
//! handlers.
//!
//! Read-side operations (`get_objects`, `get_state`, `get_changes`,
//! `query_objects`, `query_changes`) are defined on the
//! [`jmap_server::JmapBackend`] supertrait. This module adds the write
//! operations and the deployment facts the handlers consult.
//!
//! The crate root re-exports this module's public items and the
//! [`jmap_server`] backend types a downstream impl needs.

use jmap_server::{BackendSetError, JmapBackend, JmapObject, SetObject};

use jmap_scene_types::{
    Orientation, Position, SceneAsset, SceneAvatar, SceneCapability, SceneInteractionEvent,
    SceneObject, SceneRegion, SceneRegionEvent,
};

/// A SceneRegion binding property, the reference
/// [`SceneBackend::region_binding`] is asked about
/// (draft-atwood-jmap-scene-00 §Creating a Region, §Updating a Region).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindingProperty {
    /// `chatId`, a Chat (`urn:ietf:params:jmap:chat`).
    ChatId,
    /// `spaceId`, a Space (`urn:ietf:params:jmap:chat`).
    SpaceId,
    /// `channelId`, a channel of a Space (`urn:ietf:params:jmap:chat`).
    ChannelId,
    /// `activeCallId`, a VTCCall (`urn:ietf:params:jmap:vtc`).
    ActiveCallId,
}

impl BindingProperty {
    /// The SceneRegion property name, e.g. `"activeCallId"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ChatId => "chatId",
            Self::SpaceId => "spaceId",
            Self::ChannelId => "channelId",
            Self::ActiveCallId => "activeCallId",
        }
    }
}

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

/// What [`SceneBackend::avatar_view`] lets a caller see of one
/// SceneAvatar (draft-atwood-jmap-scene-00 at jmap-chat-spec 93f2258,
/// §Avatar Visibility): "The server hides an avatar from a caller as a
/// whole, or withholds some of its properties".
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Default)]
pub enum AvatarView {
    /// The whole avatar, as stored. The default.
    #[default]
    Full,
    /// The avatar with the [`AvatarRedaction`]'s properties withheld.
    Redacted(AvatarRedaction),
    /// "A hidden avatar is treated as if it did not exist:
    /// `SceneAvatar/get` reports it in `notFound`, `SceneAvatar/query` does
    /// not return it, and a `SceneAvatar/set` update of it returns
    /// `notFound`."
    Hidden,
}

/// The properties of one SceneAvatar withheld from a caller
/// (jmap-chat-spec 93f2258 §Avatar Visibility): "Only `position`,
/// `orientation`, `worldState`, and `regionState` can be withheld."
///
/// For `position` and `orientation` the deployment supplies the value
/// returned instead, one "that tells the caller nothing it may not know,
/// such as the region's `spawnPosition` and `spawnOrientation`, or the
/// values at the time the avatar was last visible to the caller". For
/// `worldState` and `regionState` "the server leaves out the withheld
/// top-level keys". The default withholds nothing.
///
/// ```
/// use jmap_scene_server::AvatarRedaction;
///
/// let mut redaction = AvatarRedaction::default();
/// redaction.position = Some([0.0, 0.0, 5.0]);
/// redaction.region_state_keys = vec!["money".to_owned()];
/// assert_eq!(redaction.withheld(), ["position", "regionState"]);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AvatarRedaction {
    /// The `position` returned instead of the stored one; `None` to
    /// return the stored one.
    pub position: Option<Position>,
    /// The `orientation` returned instead of the stored one; `None` to
    /// return the stored one.
    pub orientation: Option<Orientation>,
    /// The top-level `worldState` keys left out.
    pub world_state_keys: Vec<String>,
    /// The top-level `regionState` keys left out.
    pub region_state_keys: Vec<String>,
}

impl AvatarRedaction {
    /// Apply this redaction to `avatar`: replace `position` and
    /// `orientation` where set, and remove the listed `worldState` and
    /// `regionState` keys. A `null` or absent state object is left as it
    /// is.
    pub fn apply(&self, avatar: &mut SceneAvatar) {
        if let Some(p) = self.position {
            avatar.position = p;
        }
        if let Some(o) = self.orientation {
            avatar.orientation = o;
        }
        if let Some(ws) = &mut avatar.world_state {
            for k in &self.world_state_keys {
                ws.remove(k);
            }
        }
        if let Some(Some(rs)) = &mut avatar.region_state {
            for k in &self.region_state_keys {
                rs.remove(k);
            }
        }
    }

    /// The SceneAvatar property names this redaction withholds, in the
    /// order `position`, `orientation`, `worldState`, `regionState`.
    pub fn withheld(&self) -> Vec<&'static str> {
        [
            ("position", self.position.is_some()),
            ("orientation", self.orientation.is_some()),
            ("worldState", !self.world_state_keys.is_empty()),
            ("regionState", !self.region_state_keys.is_empty()),
        ]
        .into_iter()
        .filter_map(|(name, on)| on.then_some(name))
        .collect()
    }
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
/// # Changes
///
/// Unlike `/get` and `/query`, [`JmapReads::get_changes`](jmap_server::JmapReads::get_changes) is not scoped to
/// the caller. Visibility filtering "does not change the JMAP state model:
/// each data type has one state string per account", and `/changes` reports
/// the account's changes (draft-atwood-jmap-scene-00 §Visibility Contract).
/// A `/changes` result may therefore name an id that `/get` then hides: "A
/// server MAY report an object hidden from the caller in `notFound` from
/// `/get`". Unlike draft-atwood-jmap-graph-00, Scene does not ask `/changes`
/// to omit what the caller may not read, with one exception: a backend
/// that hides SceneAvatars or withholds their properties to protect
/// information ([`Self::avatar_view`]) SHOULD answer `SceneAvatar/changes`
/// from the caller's view (jmap-chat-spec 93f2258 §Avatar Visibility).
/// `get_changes` receives the caller for that.
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
    /// The handler has validated the create against
    /// draft-atwood-jmap-scene-00 §SceneAsset/set and filled the server-set
    /// properties other than `id`. The backend MUST, in one atomic step,
    /// mint a ULID `id` ("A ULID assigned at creation"), call `build(&id)`
    /// once for the object carrying it, and store that object.
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
    fn create_object<O, F>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        create_id: &str,
        build: F,
    ) -> impl std::future::Future<Output = Result<(jmap_types::Id, O), BackendSetError<Self::Error>>>
    + Send
    where
        O: SetObject + Send + Sync,
        F: FnOnce(&jmap_types::Id) -> O + Send;

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

    /// What the binding `property` set to `id` references, for the checks of
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
        property: BindingProperty,
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
    /// The same owner-or-admin rule gates `SceneRegion/broadcast` and
    /// changes to `SceneAvatar.regionState` on the region's avatars
    /// (jmap-chat-spec 93f2258), so those handlers ask this hook too.
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
    /// Identity and Impersonation, jmap-chat-spec 77c3474 L2776): "The
    /// server MAY maintain a list of reserved display names that correspond
    /// to system roles or administrative functions and MAY reject
    /// `SceneAvatar/set` operations that attempt to use a reserved name with
    /// a `SetError` of type `forbidden`." This crate rejects them by choice.
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
    /// "The server SHOULD treat a request from a user without access to a
    /// region as if the region did not exist: `notFound` for
    /// `SceneRegion/get`, `SceneObject/get`, and `SceneAvatar/get` ... and
    /// no results from the region in `/query`" (draft-atwood-jmap-scene-00
    /// at 29e9a41, L2606).
    /// `SceneRegion/set`, `SceneObject/set` and `SceneAvatar/set` report an
    /// update or destroy of such a record as `notFound` too, never
    /// `forbidden`. A region that no longer exists
    /// may be treated either way.
    ///
    /// The handlers cannot filter a `/query` page they did not cut, so
    /// `query_objects` should omit what `/get` would hide: "The server
    /// SHOULD NOT include a SceneObject in a `SceneObject/get` response or
    /// `SceneObject/query` result if the authenticated user does not have
    /// access to the object's containing SceneRegion" (§Visibility
    /// Contract, L2577). The same applies to SceneRegion and SceneAvatar
    /// results (L2606).
    ///
    /// `query_changes` is not filtered per caller: visibility filtering
    /// "does not change the JMAP state model: each data type has one state
    /// string per account ... and `/changes` and `/queryChanges` report
    /// changes for the account" (L2587). An id it reports that the caller
    /// may not see, `/get` reports in `notFound`.
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
    /// update or destroy of it with `notFound`: "The server SHOULD NOT
    /// return `forbidden`, as that would confirm the object still exists"
    /// (L2589). `query_objects` should omit such objects as well;
    /// `query_changes` reports changes for the account (L2587).
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

    /// `true` if `user_id` is blocked on the caller's own contact list
    /// (draft-atwood-jmap-scene-00 at 33702a5 §Blocked-Contact Spatial
    /// Presence, L2672): "SceneAvatar records belonging to a blocked
    /// ChatContact SHOULD be excluded from `SceneAvatar/get` responses and
    /// `SceneAvatar/query` results delivered to the blocking user".
    ///
    /// The list is the caller's, whatever account the request names: the
    /// server "removes any SceneAvatar records whose `userId` corresponds
    /// to a ChatContact with `blocked: true` on the requesting user's
    /// contact list" (L2676). A request on a shared region names the
    /// region's hosting account, which holds the host's contacts, not the
    /// requester's, so the hook takes no account id; an implementation
    /// resolves the caller's own account from `caller`.
    ///
    /// Consulted only for avatars in a region the caller may access:
    /// "Blocked-contact filtering is applied AFTER the visibility
    /// filtering". `SceneAvatar/get` reports such an avatar exactly as a
    /// missing one, in `notFound` ("The server SHOULD NOT return an error,
    /// a filtered-result indicator, or any other signal", L2674);
    /// `query_objects` should omit it as well, while `query_changes`
    /// reports changes for the account (§Visibility Contract). The
    /// handlers leave `activeAvatarCount` the true count, which deployments
    /// "SHOULD prefer" (L2678).
    ///
    /// [`crate::should_deliver`] asks this at delivery time with the
    /// recipient as the caller, since suppression consults "the
    /// recipient's ChatContact records" (draft-atwood-jmap-scene-wss-00 at
    /// 33702a5, L793-795).
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
        user_id: &str,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        let _ = (caller, user_id);
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
    /// `query_objects` should omit them as well, while `query_changes`
    /// reports changes for the account (§Visibility Contract).
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

    /// What the caller may see of `avatar`, another user's avatar
    /// (draft-atwood-jmap-scene-00 at jmap-chat-spec 93f2258, §Avatar
    /// Visibility): "A server MAY also limit which SceneAvatar records, and
    /// which of their properties, each caller sees: for example, to keep
    /// opponents' positions from players during a competitive round". The
    /// rule is deployment-defined.
    ///
    /// The handlers never ask about the caller's own avatar: "A caller
    /// always sees their own avatar in full". The hook "SHOULD NOT hide
    /// avatars, or withhold their properties, from the region owner and
    /// administrators", which is the implementation's to honor. It is
    /// consulted after region access, the blocked-contact filter and
    /// [`Self::may_see_region_avatars`], which stay separate.
    ///
    /// - `SceneAvatar/get` reports an [`AvatarView::Hidden`] avatar in
    ///   `notFound` and returns a [`AvatarView::Redacted`] one with its
    ///   [`AvatarRedaction`] applied; the withheld properties are
    ///   unreadable to a draft-ietf-jmap-conditional-02 precondition.
    /// - `SceneAvatar/set` refuses an update of a hidden avatar with
    ///   `notFound`.
    /// - [`crate::should_deliver`] drops a `SceneAvatarEvent` about a
    ///   hidden avatar: "an event about an avatar hidden from the
    ///   recipient is not delivered to that recipient".
    /// - `query_objects` must apply the same view, since the handlers
    ///   cannot filter a page they did not cut: leave hidden avatars out
    ///   and evaluate filters and sorts "against the values returned to
    ///   the caller, so a spatial query cannot locate an avatar whose
    ///   position is withheld".
    /// - `get_changes` (and `query_changes`) of a backend that hides
    ///   avatars "to protect information, rather than only for blocked
    ///   contacts or end-to-end encrypted calls, SHOULD answer `/changes`
    ///   and `/queryChanges` from the caller's view": an avatar that
    ///   becomes visible is `created`, one that becomes hidden is
    ///   `destroyed`, and one is `updated` "only when a value returned to
    ///   the caller changes". The state string stays one per account and
    ///   advances "Whenever the caller's view changes", so push notifies the
    ///   caller. The handlers pass the caller to `get_changes`; the
    ///   per-caller log is the backend's.
    ///
    /// `activeAvatarCount` still counts hidden avatars.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(AvatarView::Full)`: every avatar is visible in full,
    /// today's behavior (fail-open, bd:JMAP-bghk).
    fn avatar_view(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        avatar: &SceneAvatar,
    ) -> impl std::future::Future<Output = Result<AvatarView, Self::Error>> + Send {
        let _ = (caller, account_id, avatar);
        async { Ok(AvatarView::Full) }
    }

    /// `true` if the server advertises `supportsObserve` in its
    /// `urn:ietf:params:jmap:scene:websocket` Session capability
    /// ([`SceneWebSocketCapability`](jmap_scene_types::SceneWebSocketCapability);
    /// draft-atwood-jmap-scene-wss-00 at jmap-chat-spec 93f2258).
    ///
    /// When `true`, `SceneRegion/set` gives a created region
    /// `allowObservers` (default `true`) and `observerCount` (`0`) and
    /// accepts updates of `allowObservers`, and the `websocket` handler
    /// recognizes `observe` in `SceneStreamEnable`. When `false` the two
    /// region properties are refused with `invalidProperties` and
    /// `observe` is ignored: "Recognized only when the server advertises
    /// `supportsObserve`". A backend that returns `true` must also
    /// implement [`Self::set_observing`], so `observerCount` is right.
    ///
    /// # Default implementation
    ///
    /// Returns `false`: no observer subscriptions, today's behavior.
    fn supports_observe(&self, caller: &Self::CallerCtx) -> bool {
        let _ = caller;
        false
    }

    /// `true` unless the server declines observers of `region` for
    /// deployment-defined policy: "A server MAY also decline observers of
    /// a region for deployment-defined policy reasons even when
    /// `allowObservers` is `true`" (jmap-chat-spec 93f2258
    /// §SceneStreamEnable).
    ///
    /// Asked for a region the caller has access to, when a
    /// `SceneStreamEnable` with `observe: true` lists it (`false` makes it
    /// one of the refused `regionIds` of the `forbidden` `RequestError`)
    /// and again at delivery time by [`crate::should_deliver`] for an
    /// observer without an active avatar there.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(true)` (fail-open, bd:JMAP-bghk).
    fn may_observe_region(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        region: &SceneRegion,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        let _ = (caller, account_id, region);
        async { Ok(true) }
    }

    /// The accounts whose regions a `SceneStreamEnable` with
    /// `observe: true` may name, for a connection whose own account is
    /// `own_account_id`: "An id listed in `regionIds` matches in every
    /// account the user can access" (draft-atwood-jmap-scene-wss-00
    /// §SceneStreamEnable).
    ///
    /// The `websocket` handler looks each listed region up in each of
    /// these accounts to decide which regions are observed and which are
    /// refused. Access to each region is still checked with
    /// [`Self::may_access_region`].
    ///
    /// # Default implementation
    ///
    /// Returns `own_account_id` alone. A deployment with shared regions
    /// overrides it with the accounts the caller can access.
    fn observer_accounts(
        &self,
        caller: &Self::CallerCtx,
        own_account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Vec<jmap_types::Id>, Self::Error>> + Send {
        let _ = caller;
        let own = vec![own_account_id.clone()];
        async { Ok(own) }
    }

    /// Record that one connection of the caller started (`observing`
    /// `true`) or stopped (`false`) observing region `region_id` of
    /// `account_id` (jmap-chat-spec 93f2258, `observerCount`).
    ///
    /// The `websocket` handler calls it with `true` for each region a
    /// `SceneStreamEnable` with `observe: true` observes, and with `false`
    /// when the connection's subscription stops covering it: a new
    /// `SceneStreamEnable`, `SceneStreamDisable` or the connection
    /// closing. Calls are balanced per connection, so a caller with
    /// several observing connections calls `true` several times.
    ///
    /// The backend keeps `observerCount`: "The number of users currently
    /// observing the region through an observer subscription ... who do
    /// not have an active avatar in it. A user with several observing
    /// connections counts once." Recount it, atomically with the change,
    /// here and whenever one of those users enters or leaves the region
    /// with an avatar, loses access, or `allowObservers` changes (`false`
    /// stops delivery, so it counts no one). "Like `activeAvatarCount`, a
    /// change to it is a change to the SceneRegion": record the region as
    /// updated and advance the SceneRegion state, so push sends a
    /// `StateChange`.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(())` and counts nothing. That is correct only while
    /// [`Self::supports_observe`] is `false`, when no observer exists.
    fn set_observing(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        region_id: &jmap_types::Id,
        observing: bool,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send {
        let _ = (caller, account_id, region_id, observing);
        async { Ok(()) }
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

    /// Accept a region event that passed the `SceneRegion/broadcast`
    /// checks, or refuse it for its rate (draft-atwood-jmap-scene-00 at
    /// jmap-chat-spec 93f2258, §SceneRegion/broadcast).
    ///
    /// The handler calls this only when the account's
    /// [`SceneCapability::supports_region_broadcast`] is `true`, the caller
    /// has access to the region ([`Self::may_access_region`]) and is its
    /// owner or an administrator ([`Self::may_update_region`]: "Only the
    /// region owner and users with deployment-defined administrative
    /// privileges may broadcast to a region"). `Ok(false)` means "The
    /// caller has exceeded a deployment-defined rate for region events" and
    /// the handler reports `rateLimit`. `Ok(true)` accepts it: count it
    /// against the rate and, when the WebSocket capability is supported,
    /// hand `event` to the transport, which "delivers a `SceneRegionEvent`
    /// to the WebSocket subscribers of the region", filtered per recipient
    /// by [`crate::should_deliver`] (with the `websocket` feature,
    /// `websocket::SceneEventHub::publish`).
    ///
    /// Server-originated region events ("The server itself MAY also
    /// originate region events") do not pass through here; the deployment
    /// publishes them to the transport directly.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(true)` and delivers nothing: no rate limit (fail-open,
    /// bd:JMAP-bghk) and no WebSocket transport.
    fn accept_region_event(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        event: &SceneRegionEvent,
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
