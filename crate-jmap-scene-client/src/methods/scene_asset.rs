//! JMAP Scene — `SceneAsset/*` method implementations on SessionClient
//! (draft-atwood-jmap-scene-00 §SceneAsset Methods).

use std::collections::BTreeMap;

use jmap_scene_types::SceneAsset;
use jmap_scene_types::SceneAssetComparator;
use jmap_types::{Filter, Id, PatchObject, State};
use serde::Serialize;

use super::{
    ChangesResponse, GetResponse, PartialObject, QueryChangesResponse, QueryResponse, SetResponse,
};

impl super::SessionClient {
    /// `SceneAsset/get` (draft-atwood-jmap-scene-00 L674-L676; RFC 8620 §5.1).
    ///
    /// `ids: None` asks for every SceneAsset in the account, subject to the
    /// server's `maxObjectsInGet` cap. `properties: None` returns all
    /// properties; a `properties` subset that omits a required
    /// [`SceneAsset`] field fails to decode, so request full objects.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:scene`.
    /// - [`ClientError::Serialize`](jmap_base_client::ClientError::Serialize)
    ///   if serializing a caller-supplied value fails.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call), including
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError) for
    ///   RFC 8620 §3.6.2 method-level errors.
    pub async fn scene_asset_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<SceneAsset>, jmap_base_client::ClientError> {
        self.standard_get("SceneAsset/get", ids, properties).await
    }

    /// `SceneAsset/changes` (draft-atwood-jmap-scene-00 L678-L680; RFC 8620 §5.2).
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
    pub async fn scene_asset_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        self.standard_changes("SceneAsset/changes", since_state, max_changes)
            .await
    }

    /// `SceneAsset/set` (draft-atwood-jmap-scene-00 L682-L870; RFC 8620 §5.3).
    ///
    /// Creates the record for a blob uploaded through the JMAP upload
    /// endpoint, patches `name`, `assetUri` and `sha256`, and destroys the
    /// record; the blob itself follows RFC 8620 §6 lifetime rules (L686-L700).
    ///
    /// The result is the untyped [`SetResponse`]: per RFC 8620 §5.3 a
    /// `created` entry carries only the server-set properties, which is not
    /// a whole [`SceneAsset`].
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
    ///   if `create`, `update` and `destroy` are all `None`.
    /// - [`ClientError::Serialize`](jmap_base_client::ClientError::Serialize)
    ///   if serializing a caller-supplied value fails.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call), including
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError) for
    ///   RFC 8620 §3.6.2 method-level errors.
    pub async fn scene_asset_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<BTreeMap<Id, PatchObject>>,
        destroy: Option<Vec<Id>>,
        if_in_state: Option<&State>,
    ) -> Result<SetResponse<PartialObject>, jmap_base_client::ClientError> {
        self.standard_set("SceneAsset/set", create, update, destroy, if_in_state)
            .await
    }

    /// `SceneAsset/query` (draft-atwood-jmap-scene-00 L872-L913; RFC 8620 §5.5).
    ///
    /// Pass `None` to omit `filter` or `sort`; the default sort is
    /// `createdAt` descending (L896).
    ///
    /// `filter`'s condition type `C` is normally
    /// [`SceneAssetFilterCondition`](jmap_scene_types::SceneAssetFilterCondition);
    /// name the type when passing `None` (`None::<&SceneAssetFilter>`).
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:scene`.
    /// - [`ClientError::Serialize`](jmap_base_client::ClientError::Serialize)
    ///   if serializing a caller-supplied value fails.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call), including
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError) for
    ///   RFC 8620 §3.6.2 method-level errors.
    pub async fn scene_asset_query<C: Serialize>(
        &self,
        filter: Option<&Filter<C>>,
        sort: Option<&[SceneAssetComparator]>,
        params: &jmap_base_client::QueryParams,
    ) -> Result<QueryResponse, jmap_base_client::ClientError> {
        self.standard_query("SceneAsset/query", filter, sort, params)
            .await
    }

    /// `SceneAsset/queryChanges` (draft-atwood-jmap-scene-00 L915-L917; RFC 8620 §5.6).
    ///
    /// `filter` and `sort` must match the `SceneAsset/query` call that returned
    /// `since_query_state`.
    ///
    /// `filter`'s condition type `C` is normally
    /// [`SceneAssetFilterCondition`](jmap_scene_types::SceneAssetFilterCondition);
    /// name the type when passing `None` (`None::<&SceneAssetFilter>`).
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:scene`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `since_query_state` is empty.
    /// - [`ClientError::Serialize`](jmap_base_client::ClientError::Serialize)
    ///   if serializing a caller-supplied value fails.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call), including
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError) for
    ///   RFC 8620 §3.6.2 method-level errors.
    pub async fn scene_asset_query_changes<C: Serialize>(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
        filter: Option<&Filter<C>>,
        sort: Option<&[SceneAssetComparator]>,
        up_to_id: Option<&Id>,
        calculate_total: Option<bool>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        self.standard_query_changes(
            "SceneAsset/queryChanges",
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
