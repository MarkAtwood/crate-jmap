//! JMAP Chat — ChatContact/* method implementations on SessionClient.
//!
//! Spec: JMAP Chat draft §5 (ChatContact/get, /changes, /set, /query, /queryChanges).

use jmap_types::{Filter, Id, PatchObject, State};
use serde::Serialize;

use super::{
    ChangesResponse, ChatContactPatch, GetResponse, PartialObject, QueryChangesResponse,
    QueryResponse, SetResponse,
};

impl super::SessionClient {
    /// Fetch ChatContact objects by IDs (JMAP Chat §5 ChatContact/get).
    ///
    /// If `ids` is `None`, returns all ChatContacts for the account.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:chat`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call):
    ///   [`Http`](jmap_base_client::ClientError::Http),
    ///   [`Parse`](jmap_base_client::ClientError::Parse),
    ///   [`AuthFailed`](jmap_base_client::ClientError::AuthFailed),
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError)
    ///   (wraps RFC 8620 §3.6.2 method-level errors such as
    ///   `accountNotFound`, `invalidArguments`, `serverFail`),
    ///   [`MethodNotFound`](jmap_base_client::ClientError::MethodNotFound),
    ///   [`ResponseTooLarge`](jmap_base_client::ClientError::ResponseTooLarge),
    ///   or
    ///   [`UnexpectedResponse`](jmap_base_client::ClientError::UnexpectedResponse).
    pub async fn chat_contact_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_chat_types::ChatContact>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        // Omit `ids` / `properties` when None — see the matching comment on
        // `chat_get` for the rationale (consistent with set/changes/query).
        let mut args = serde_json::json!({ "accountId": account_id });
        if let Some(id_slice) = ids {
            args["ids"] = serde_json::to_value(id_slice)
                .map_err(jmap_base_client::ClientError::from_parse)?;
        }
        if let Some(props) = properties {
            args["properties"] =
                serde_json::to_value(props).map_err(jmap_base_client::ClientError::from_parse)?;
        }
        let req = super::build_request("ChatContact/get", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to ChatContact objects since `since_state` (RFC 8620 §5.2).
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `since_state` is the empty string (defence-in-depth —
    ///   `State` constructed via [`State::from`](jmap_types::State::from)
    ///   accepts empty strings, but an empty `sinceState` is never
    ///   useful and would otherwise generate a wasted round-trip).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:chat`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::chat_contact_get`].
    pub async fn chat_contact_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `chat_changes`.
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "chat_contact_changes: since_state may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "sinceState": since_state,
        });
        if let Some(mc) = max_changes {
            args["maxChanges"] = mc.into();
        }
        let req = super::build_request("ChatContact/changes", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create a ChatContact for `identity` (JMAP Chat §ChatContact/set
    /// create).
    ///
    /// `identity` is the only property a create carries; the server
    /// assigns `id` and sets the other server-set properties. A ULID is
    /// generated as the creation id.
    ///
    /// Pass `if_in_state: Some(&state)` to use an optimistic-concurrency guard
    /// (RFC 8620 §5.3 `ifInState`): the server rejects the whole call with
    /// `stateMismatch` unless its current state matches.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:chat`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::chat_contact_get`]. /set
    ///   create errors appear in [`SetResponse::not_created`] rather
    ///   than as [`Err`]: `alreadyExists` (with `existingId`) when a
    ///   ChatContact with that `identity` exists, and
    ///   `invalidProperties` when `identity` is the owner's.
    pub async fn chat_contact_create(
        &self,
        identity: &str,
        if_in_state: Option<&State>,
    ) -> Result<SetResponse<PartialObject>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let client_id = super::resolve_client_id(None);
        let mut args = serde_json::json!({
            "accountId": account_id,
            "create": { client_id: { "identity": identity } },
        });
        if let Some(s) = if_in_state {
            args["ifInState"] = s.as_ref().into();
        }
        let req = super::build_request("ChatContact/set", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Update ChatContact properties (JMAP Chat §ChatContact/set).
    ///
    /// Supports `blocked` (Boolean) and `displayName` (nullable String).
    /// Create is [`Self::chat_contact_create`]; destroy is not supported
    /// by spec and the server returns `forbidden`.
    ///
    /// Pass `if_in_state: Some(&state)` to use an optimistic-concurrency guard
    /// (RFC 8620 §5.3 `ifInState`): the server rejects the whole call with
    /// `stateMismatch` unless its current state matches.
    ///
    /// # Errors
    ///
    /// - [`ClientError::Parse`](jmap_base_client::ClientError::Parse) if
    ///   serializing the typed `display_name` Clearable entry fails
    ///   (pathological conditions only).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:chat`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::chat_contact_get`]. /set
    ///   update errors appear in
    ///   [`SetResponse::not_updated`] rather than as
    ///   [`Err`].
    pub async fn chat_contact_update(
        &self,
        id: &Id,
        patch: &ChatContactPatch<'_>,
        if_in_state: Option<&State>,
    ) -> Result<SetResponse<PartialObject>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut patch_map = serde_json::Map::new();
        if let Some(b) = patch.blocked {
            patch_map.insert("blocked".into(), b.into());
        }
        if let Some(entry) = patch
            .display_name
            .map_entry()
            .map_err(jmap_base_client::ClientError::from_parse)?
        {
            patch_map.insert("displayName".into(), entry);
        }
        // Wrap the constructed map in a PatchObject (RFC 8620 §5.3) before
        // serializing. Wire bytes are unchanged because PatchObject is
        // #[serde(transparent)]; the typed boundary documents the contract.
        let patch_value = serde_json::Value::Object(PatchObject::from_map(patch_map).into_inner());
        let mut args = serde_json::json!({
            "accountId": account_id,
            "update": { id.as_ref(): patch_value },
        });
        if let Some(s) = if_in_state {
            args["ifInState"] = s.as_ref().into();
        }
        let req = super::build_request("ChatContact/set", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Query ChatContact IDs (RFC 8620 §5.5 / JMAP Chat §ChatContact/query).
    ///
    /// `filter` is an RFC 8620 §5.5 `Filter`: a bare condition
    /// (`Filter::Condition`) or a `FilterOperator` combining sub-filters
    /// with `AND` / `OR` / `NOT`. The condition type `C` is normally
    /// [`ChatContactFilterCondition`](jmap_chat_types::ChatContactFilterCondition); use
    /// `jmap_metadata_types::WithMetadata<ChatContactFilterCondition>` to add the
    /// draft-ietf-jmap-metadata-04 §3.6 `metadata*` conditions.
    /// Pass `None` to send `"filter": null`; name the type when you do (`None::<&ChatContactFilter>`).
    ///
    /// # Errors
    ///
    /// - [`ClientError::Parse`](jmap_base_client::ClientError::Parse) if
    ///   serializing `filter` or `sort` fails (a caller-supplied condition
    ///   type `C` whose `Serialize` impl errors).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:chat`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::chat_contact_get`]. RFC 8620 §5.5
    ///   defines additional /query method-level errors
    ///   (`anchorNotFound`, `unsupportedFilter`, `unsupportedSort`,
    ///   `tooManyChanges`) that surface as
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError).
    pub async fn chat_contact_query<C: Serialize>(
        &self,
        filter: Option<&Filter<C>>,
        sort: Option<&[jmap_chat_types::ChatContactComparator]>,
        params: &jmap_base_client::QueryParams,
    ) -> Result<QueryResponse, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = params.to_arguments(account_id);
        args["filter"] =
            serde_json::to_value(filter).map_err(jmap_base_client::ClientError::from_parse)?;
        if let Some(s) = sort {
            args["sort"] =
                serde_json::to_value(s).map_err(jmap_base_client::ClientError::from_parse)?;
        }
        let req = super::build_request("ChatContact/query", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch query-result changes for ChatContact since `since_query_state`
    /// (RFC 8620 §5.6 / ChatContact/queryChanges).
    ///
    /// `filter` and `sort` MUST match the `filter` / `sort` passed to the
    /// original `ChatContact/query` call that returned `since_query_state`
    /// — RFC 8620 §5.6 is explicit that the server uses them to compute
    /// which entries entered or left the result set.
    ///
    /// `up_to_id` is the highest-index id the client has cached;
    /// `calculate_total` requests the new total result count.
    ///
    /// `filter` is an RFC 8620 §5.5 `Filter`: a bare condition
    /// (`Filter::Condition`) or a `FilterOperator` combining sub-filters
    /// with `AND` / `OR` / `NOT`. The condition type `C` is normally
    /// [`ChatContactFilterCondition`](jmap_chat_types::ChatContactFilterCondition); use
    /// `jmap_metadata_types::WithMetadata<ChatContactFilterCondition>` to add the
    /// draft-ietf-jmap-metadata-04 §3.6 `metadata*` conditions.
    /// Pass `None` to omit it; name the type when you do (`None::<&ChatContactFilter>`).
    ///
    /// # Errors
    ///
    /// - [`ClientError::Parse`](jmap_base_client::ClientError::Parse) if
    ///   serializing `filter` fails (a caller-supplied condition type `C`
    ///   whose `Serialize` impl errors).
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `since_query_state` is the empty string (defence-in-depth
    ///   empty-state guard; see [`Self::chat_contact_changes`]).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:chat`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::chat_contact_get`]. RFC
    ///   8620 §5.6 also defines `cannotCalculateChanges` (returned when
    ///   the server cannot honour the request given the supplied
    ///   filter / sort); it surfaces as
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError).
    pub async fn chat_contact_query_changes<C: Serialize>(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
        filter: Option<&Filter<C>>,
        sort: Option<&[jmap_chat_types::ChatContactComparator]>,
        up_to_id: Option<&Id>,
        calculate_total: Option<bool>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `chat_changes`.
        if since_query_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "chat_contact_query_changes: since_query_state may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "sinceQueryState": since_query_state,
        });
        if let Some(f) = filter {
            args["filter"] =
                serde_json::to_value(f).map_err(jmap_base_client::ClientError::from_parse)?;
        }
        if let Some(s) = sort {
            args["sort"] =
                serde_json::to_value(s).map_err(jmap_base_client::ClientError::from_parse)?;
        }
        if let Some(mc) = max_changes {
            args["maxChanges"] = mc.into();
        }
        if let Some(uti) = up_to_id {
            args["upToId"] =
                serde_json::to_value(uti).map_err(jmap_base_client::ClientError::from_parse)?;
        }
        if let Some(ct) = calculate_total {
            args["calculateTotal"] = ct.into();
        }
        let req = super::build_request("ChatContact/queryChanges", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}
