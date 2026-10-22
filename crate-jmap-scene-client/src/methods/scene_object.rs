//! JMAP Scene — `SceneObject/*` method implementations on SessionClient
//! (draft-atwood-jmap-scene-00 §SceneObject Methods).

use std::collections::BTreeMap;

use jmap_scene_types::query::{SceneObjectComparator, SceneObjectFilter};
use jmap_scene_types::SceneObject;
use jmap_types::{Id, PatchObject, State};

use super::{ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SetResponse};

impl super::SessionClient {
    /// `SceneObject/get` (draft-atwood-jmap-scene-00 L1446-L1530; RFC 8620 §5.1).
    ///
    /// `ids: None` asks for every SceneObject in the account, subject to the
    /// server's `maxObjectsInGet` cap. `properties: None` returns all
    /// properties; a `properties` subset that omits a required
    /// [`SceneObject`] field fails to decode, so request full objects.
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
    pub async fn scene_object_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<SceneObject>, jmap_base_client::ClientError> {
        self.standard_get("SceneObject/get", ids, properties).await
    }

    /// `SceneObject/changes` (draft-atwood-jmap-scene-00 L1532-L1534; RFC 8620 §5.2).
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
    pub async fn scene_object_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        self.standard_changes("SceneObject/changes", since_state, max_changes)
            .await
    }

    /// `SceneObject/set` (draft-atwood-jmap-scene-00 L1536-L1856; RFC 8620 §5.3).
    ///
    /// Creates, updates and destroys objects in a region; a create needs
    /// `regionId` (L1544).
    ///
    /// The result is the untyped [`SetResponse`]: per RFC 8620 §5.3 a
    /// `created` entry carries only the server-set properties, which is not
    /// a whole [`SceneObject`].
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
    pub async fn scene_object_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<BTreeMap<Id, PatchObject>>,
        destroy: Option<Vec<Id>>,
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        self.standard_set("SceneObject/set", create, update, destroy)
            .await
    }

    /// `SceneObject/query` (draft-atwood-jmap-scene-00 L1858-L2009; RFC 8620 §5.5).
    ///
    /// The filter must set `regionId`: "Servers MUST return
    /// `invalidArguments` when this property is absent" (L1866-L1867). The
    /// default sort is `createdAt` ascending (L1912).
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
    pub async fn scene_object_query(
        &self,
        filter: Option<&SceneObjectFilter>,
        sort: Option<&[SceneObjectComparator]>,
        params: &jmap_base_client::QueryParams,
    ) -> Result<QueryResponse, jmap_base_client::ClientError> {
        self.standard_query("SceneObject/query", filter, sort, params)
            .await
    }

    /// `SceneObject/queryChanges` (draft-atwood-jmap-scene-00 L2011-L2013; RFC 8620 §5.6).
    ///
    /// `filter` and `sort` must match the `SceneObject/query` call that returned
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
    pub async fn scene_object_query_changes(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
        filter: Option<&SceneObjectFilter>,
        sort: Option<&[SceneObjectComparator]>,
        up_to_id: Option<&Id>,
        calculate_total: Option<bool>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        self.standard_query_changes(
            "SceneObject/queryChanges",
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
