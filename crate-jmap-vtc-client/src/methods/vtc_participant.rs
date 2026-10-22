//! JMAP VTC — `VTCParticipant/*` method implementations on SessionClient
//! (draft-atwood-jmap-vtc-00 §VTCParticipant Methods).

use std::collections::BTreeMap;

use jmap_types::{Id, PatchObject, State};
use jmap_vtc_types::VtcParticipant;
use jmap_vtc_types::{VtcParticipantComparator, VtcParticipantFilter};

use super::{
    ChangesResponse, GetResponse, PartialObject, QueryChangesResponse, QueryResponse, SetResponse,
};

impl super::SessionClient {
    /// `VTCParticipant/get` (draft-atwood-jmap-vtc-00 L824-L826; RFC 8620 §5.1).
    ///
    /// `ids: None` asks for every VTCParticipant in the account, subject to the
    /// server's `maxObjectsInGet` cap. `properties: None` returns all
    /// properties; a `properties` subset that omits a required
    /// [`VtcParticipant`] field fails to decode, so request full objects.
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
    pub async fn vtc_participant_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<VtcParticipant>, jmap_base_client::ClientError> {
        self.standard_get("VTCParticipant/get", ids, properties)
            .await
    }

    /// `VTCParticipant/changes` (draft-atwood-jmap-vtc-00 L828-L830; RFC 8620 §5.2).
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
    pub async fn vtc_participant_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        self.standard_changes("VTCParticipant/changes", since_state, max_changes)
            .await
    }

    /// `VTCParticipant/set` (draft-atwood-jmap-vtc-00 L832-L1068; RFC 8620 §5.3).
    ///
    /// Joins a room or scheduled call (`create` with `callId` and `joinMethod`),
    /// answers, declines or leaves a ring call, updates `mediaState`, applies
    /// the participant-level moderator actions and dials out via a gateway.
    ///
    /// The result is the untyped [`SetResponse`]: per RFC 8620 §5.3 a
    /// `created` entry carries only the server-set properties (the draft's
    /// own example responses do), which is not a whole [`VtcParticipant`].
    ///
    /// Pass `if_in_state: Some(&state)` to use an optimistic-concurrency guard
    /// (RFC 8620 §5.3 `ifInState`): the server rejects the whole call with
    /// `stateMismatch` unless its current state matches.
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
    pub async fn vtc_participant_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<BTreeMap<Id, PatchObject>>,
        destroy: Option<Vec<Id>>,
        if_in_state: Option<&State>,
    ) -> Result<SetResponse<PartialObject>, jmap_base_client::ClientError> {
        self.standard_set("VTCParticipant/set", create, update, destroy, if_in_state)
            .await
    }

    /// `VTCParticipant/query` (draft-atwood-jmap-vtc-00 L1070-L1096; RFC 8620 §5.5).
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
    pub async fn vtc_participant_query(
        &self,
        filter: Option<&VtcParticipantFilter>,
        sort: Option<&[VtcParticipantComparator]>,
        params: &jmap_base_client::QueryParams,
    ) -> Result<QueryResponse, jmap_base_client::ClientError> {
        self.standard_query("VTCParticipant/query", filter, sort, params)
            .await
    }

    /// `VTCParticipant/queryChanges` (draft-atwood-jmap-vtc-00 L1098-L1100; RFC 8620 §5.6).
    ///
    /// `filter` and `sort` must match the `VTCParticipant/query` call that returned
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
    pub async fn vtc_participant_query_changes(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
        filter: Option<&VtcParticipantFilter>,
        sort: Option<&[VtcParticipantComparator]>,
        up_to_id: Option<&Id>,
        calculate_total: Option<bool>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        self.standard_query_changes(
            "VTCParticipant/queryChanges",
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
