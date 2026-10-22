//! JMAP VTC — `VTCRecording/*` method implementations on SessionClient
//! (draft-atwood-jmap-vtc-00 §VTCRecording Methods).

use std::collections::BTreeMap;

use jmap_types::{Id, PatchObject, State};
use jmap_vtc_types::query::{VtcMediaComparator, VtcRecordingFilter};
use jmap_vtc_types::VtcRecording;

use super::{ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SetResponse};

impl super::SessionClient {
    /// `VTCRecording/get` (draft-atwood-jmap-vtc-00 L1104-L1108; RFC 8620 §5.1).
    ///
    /// `ids: None` asks for every VTCRecording in the account, subject to the
    /// server's `maxObjectsInGet` cap. `properties: None` returns all
    /// properties; a `properties` subset that omits a required
    /// [`VtcRecording`] field fails to decode, so request full objects.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:vtc`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if serializing an argument fails.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call), including
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError) for
    ///   RFC 8620 §3.6.2 method-level errors.
    pub async fn vtc_recording_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<VtcRecording>, jmap_base_client::ClientError> {
        self.standard_get("VTCRecording/get", ids, properties).await
    }

    /// `VTCRecording/changes` (draft-atwood-jmap-vtc-00 L1110-L1112; RFC 8620 §5.2).
    ///
    /// If `has_more_changes` is true in the response, call again with
    /// `new_state` as `since_state` until it is false.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:vtc`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `since_state` is empty.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call), including
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError) for
    ///   RFC 8620 §3.6.2 method-level errors.
    pub async fn vtc_recording_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        self.standard_changes("VTCRecording/changes", since_state, max_changes)
            .await
    }

    /// `VTCRecording/set` (draft-atwood-jmap-vtc-00 L1114-L1169; RFC 8620 §5.3).
    ///
    /// Starts (`create` with `callId`), pauses, resumes and stops (`update` of
    /// `state`) and destroys a recording. Moderator-only.
    ///
    /// The result is the untyped [`SetResponse`]: per RFC 8620 §5.3 a
    /// `created` entry carries only the server-set properties (the draft's
    /// own example responses do), which is not a whole [`VtcRecording`].
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:vtc`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `create`, `update` and `destroy` are all `None`, or if
    ///   serializing `update` fails.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call), including
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError) for
    ///   RFC 8620 §3.6.2 method-level errors.
    pub async fn vtc_recording_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<BTreeMap<Id, PatchObject>>,
        destroy: Option<Vec<Id>>,
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        self.standard_set("VTCRecording/set", create, update, destroy)
            .await
    }

    /// `VTCRecording/query` (draft-atwood-jmap-vtc-00 L1171-L1194; RFC 8620 §5.5).
    ///
    /// Pass `None` to omit `filter` or `sort`; the server then applies the
    /// draft's default sort.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:vtc`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if serializing an argument fails.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call), including
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError) for
    ///   RFC 8620 §3.6.2 method-level errors.
    pub async fn vtc_recording_query(
        &self,
        filter: Option<&VtcRecordingFilter>,
        sort: Option<&[VtcMediaComparator]>,
        params: &jmap_base_client::QueryParams,
    ) -> Result<QueryResponse, jmap_base_client::ClientError> {
        self.standard_query("VTCRecording/query", filter, sort, params)
            .await
    }

    /// `VTCRecording/queryChanges` (draft-atwood-jmap-vtc-00 L1196-L1198; RFC 8620 §5.6).
    ///
    /// `filter` and `sort` must match the `VTCRecording/query` call that returned
    /// `since_query_state`.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:vtc`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `since_query_state` is empty or serializing an argument fails.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call), including
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError) for
    ///   RFC 8620 §3.6.2 method-level errors.
    pub async fn vtc_recording_query_changes(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
        filter: Option<&VtcRecordingFilter>,
        sort: Option<&[VtcMediaComparator]>,
        up_to_id: Option<&Id>,
        calculate_total: Option<bool>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        self.standard_query_changes(
            "VTCRecording/queryChanges",
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
