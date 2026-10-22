//! JMAP Scene — `SceneAvatar/*` method implementations on SessionClient
//! (draft-atwood-jmap-scene-00 §SceneAvatar Methods).

use std::collections::BTreeMap;

use jmap_scene_types::query::{SceneAvatarComparator, SceneAvatarFilter};
use jmap_scene_types::SceneAvatar;
use jmap_types::{Id, PatchObject, State};

use super::{ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SetResponse};

impl super::SessionClient {
    /// `SceneAvatar/get` (draft-atwood-jmap-scene-00 L2114-L2118; RFC 8620 §5.1).
    ///
    /// `ids: None` asks for every SceneAvatar in the account, subject to the
    /// server's `maxObjectsInGet` cap. `properties: None` returns all
    /// properties; a `properties` subset that omits a required
    /// [`SceneAvatar`] field fails to decode, so request full objects.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:scene`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if serializing an argument fails.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call), including
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError) for
    ///   RFC 8620 §3.6.2 method-level errors.
    pub async fn scene_avatar_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<SceneAvatar>, jmap_base_client::ClientError> {
        self.standard_get("SceneAvatar/get", ids, properties).await
    }

    /// `SceneAvatar/changes` (draft-atwood-jmap-scene-00 L2120-L2122; RFC 8620 §5.2).
    ///
    /// If `has_more_changes` is true in the response, call again with
    /// `new_state` as `since_state` until it is false.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:scene`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `since_state` is empty.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call), including
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError) for
    ///   RFC 8620 §3.6.2 method-level errors.
    pub async fn scene_avatar_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        self.standard_changes("SceneAvatar/changes", since_state, max_changes)
            .await
    }

    /// `SceneAvatar/set` (draft-atwood-jmap-scene-00 L2124-L2460; RFC 8620 §5.3).
    ///
    /// Enters a region with a `create`, updates the caller's avatar, leaves
    /// by patching `leftAt`, and re-enters by patching `leftAt` to `null`
    /// (§Entering a Region, §Leaving a Region, §Re-entering a Region).
    ///
    /// The result is the untyped [`SetResponse`]: per RFC 8620 §5.3 a
    /// `created` entry carries only the server-set properties, which is not
    /// a whole [`SceneAvatar`].
    ///
    /// Pass `if_in_state: Some(&state)` to use an optimistic-concurrency guard
    /// (RFC 8620 §5.3 `ifInState`): the server rejects the whole call with
    /// `stateMismatch` unless its current state matches.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:scene`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `create`, `update` and `destroy` are all `None`, or if
    ///   serializing `update` fails.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call), including
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError) for
    ///   RFC 8620 §3.6.2 method-level errors.
    pub async fn scene_avatar_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<BTreeMap<Id, PatchObject>>,
        destroy: Option<Vec<Id>>,
        if_in_state: Option<&State>,
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        self.standard_set("SceneAvatar/set", create, update, destroy, if_in_state)
            .await
    }

    /// `SceneAvatar/query` (draft-atwood-jmap-scene-00 L2462-L2542; RFC 8620 §5.5).
    ///
    /// The filter must set `regionId`: "Servers MUST return
    /// `invalidArguments` when this property is absent" (L2468-L2469). The
    /// default sort is `joinedAt` ascending (L2493).
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:scene`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if serializing an argument fails.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call), including
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError) for
    ///   RFC 8620 §3.6.2 method-level errors.
    pub async fn scene_avatar_query(
        &self,
        filter: Option<&SceneAvatarFilter>,
        sort: Option<&[SceneAvatarComparator]>,
        params: &jmap_base_client::QueryParams,
    ) -> Result<QueryResponse, jmap_base_client::ClientError> {
        self.standard_query("SceneAvatar/query", filter, sort, params)
            .await
    }

    /// `SceneAvatar/queryChanges` (draft-atwood-jmap-scene-00 L2544-L2546; RFC 8620 §5.6).
    ///
    /// `filter` and `sort` must match the `SceneAvatar/query` call that returned
    /// `since_query_state`.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:scene`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `since_query_state` is empty or serializing an argument fails.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call), including
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError) for
    ///   RFC 8620 §3.6.2 method-level errors.
    pub async fn scene_avatar_query_changes(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
        filter: Option<&SceneAvatarFilter>,
        sort: Option<&[SceneAvatarComparator]>,
        up_to_id: Option<&Id>,
        calculate_total: Option<bool>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        self.standard_query_changes(
            "SceneAvatar/queryChanges",
            since_query_state,
            max_changes,
            filter,
            sort,
            up_to_id,
            calculate_total,
        )
        .await
    }
}
