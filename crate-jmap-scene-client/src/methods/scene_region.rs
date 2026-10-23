//! JMAP Scene — `SceneRegion/*` method implementations on SessionClient
//! (draft-atwood-jmap-scene-00 §SceneRegion Methods).

use std::collections::BTreeMap;

use jmap_scene_types::SceneRegion;
use jmap_scene_types::SceneRegionComparator;
use jmap_types::{Filter, Id, PatchObject, State};
use serde::Serialize;

use super::{
    ChangesResponse, GetResponse, PartialObject, QueryChangesResponse, QueryResponse, SetResponse,
};

impl super::SessionClient {
    /// `SceneRegion/get` (draft-atwood-jmap-scene-00 L921-L1037; RFC 8620 §5.1).
    ///
    /// `ids: None` asks for every SceneRegion in the account, subject to the
    /// server's `maxObjectsInGet` cap. `properties: None` returns all
    /// properties; a `properties` subset that omits a required
    /// [`SceneRegion`] field fails to decode, so request full objects.
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
    pub async fn scene_region_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<SceneRegion>, jmap_base_client::ClientError> {
        self.standard_get("SceneRegion/get", ids, properties).await
    }

    /// `SceneRegion/changes` (draft-atwood-jmap-scene-00 L1039-L1041; RFC 8620 §5.2).
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
    pub async fn scene_region_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        self.standard_changes("SceneRegion/changes", since_state, max_changes)
            .await
    }

    /// `SceneRegion/set` (draft-atwood-jmap-scene-00 L1043-L1343; RFC 8620 §5.3).
    ///
    /// Creates, updates and destroys regions. A destroy also removes every
    /// SceneObject and SceneAvatar in the region, which their `/changes`
    /// report as destroyed (L1210).
    ///
    /// The result is the untyped [`SetResponse`]: per RFC 8620 §5.3 a
    /// `created` entry carries only the server-set properties, which is not
    /// a whole [`SceneRegion`].
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
    pub async fn scene_region_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<BTreeMap<Id, PatchObject>>,
        destroy: Option<Vec<Id>>,
        if_in_state: Option<&State>,
    ) -> Result<SetResponse<PartialObject>, jmap_base_client::ClientError> {
        self.standard_set("SceneRegion/set", create, update, destroy, if_in_state)
            .await
    }

    /// `SceneRegion/query` (draft-atwood-jmap-scene-00 L1345-L1438; RFC 8620 §5.5).
    ///
    /// Pass `None` to omit `filter` or `sort`; the default sort is
    /// `createdAt` descending (L1378).
    ///
    /// `filter`'s condition type `C` is normally
    /// [`SceneRegionFilterCondition`](jmap_scene_types::SceneRegionFilterCondition);
    /// name the type when passing `None` (`None::<&SceneRegionFilter>`).
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
    pub async fn scene_region_query<C: Serialize>(
        &self,
        filter: Option<&Filter<C>>,
        sort: Option<&[SceneRegionComparator]>,
        params: &jmap_base_client::QueryParams,
    ) -> Result<QueryResponse, jmap_base_client::ClientError> {
        self.standard_query("SceneRegion/query", filter, sort, params)
            .await
    }

    /// `SceneRegion/queryChanges` (draft-atwood-jmap-scene-00 L1440-L1442; RFC 8620 §5.6).
    ///
    /// `filter` and `sort` must match the `SceneRegion/query` call that returned
    /// `since_query_state`.
    ///
    /// `filter`'s condition type `C` is normally
    /// [`SceneRegionFilterCondition`](jmap_scene_types::SceneRegionFilterCondition);
    /// name the type when passing `None` (`None::<&SceneRegionFilter>`).
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
    pub async fn scene_region_query_changes<C: Serialize>(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
        filter: Option<&Filter<C>>,
        sort: Option<&[SceneRegionComparator]>,
        up_to_id: Option<&Id>,
        calculate_total: Option<bool>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        self.standard_query_changes(
            "SceneRegion/queryChanges",
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
