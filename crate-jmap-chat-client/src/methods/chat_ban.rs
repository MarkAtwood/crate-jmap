//! JMAP Chat — ChatBan/* method implementations on SessionClient.
//!
//! Spec: draft-atwood-jmap-chat-00 §ChatBan Methods (ChatBan/get, /changes, /set).

use jmap_types::{Id, PatchObject, State};

use super::{
    ChangesResponse, ChatBanCreateInput, ChatBanPatch, GetResponse, PartialObject, SetResponse,
};

impl super::SessionClient {
    /// Fetch ChatBan objects by IDs (draft-atwood-jmap-chat-00 §ChatBan/get).
    ///
    /// If `ids` is `None`, returns all ChatBan objects for the account.
    /// Pass `properties: None` to return all fields.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:chat`.
    /// - [`ClientError::Serialize`](jmap_base_client::ClientError::Serialize)
    ///   if serializing a caller-supplied value fails.
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
    pub async fn chat_ban_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_chat_types::ChatBan>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        // Omit `ids` / `properties` when None — see the matching comment on
        // `chat_get` for the rationale (consistent with set/changes/query).
        let mut args = serde_json::json!({ "accountId": account_id });
        if let Some(id_slice) = ids {
            args["ids"] = jmap_base_client::to_args(id_slice)?;
        }
        if let Some(props) = properties {
            args["properties"] = jmap_base_client::to_args(props)?;
        }
        let req = super::build_request("ChatBan/get", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to ChatBan objects since `since_state` (RFC 8620 §5.2 / ChatBan/changes).
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `since_state` is the empty string (defence-in-depth
    ///   empty-state guard).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:chat`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::chat_ban_get`].
    pub async fn chat_ban_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `chat_changes`.
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "chat_ban_changes: since_state may not be empty".into(),
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
        let req = super::build_request("ChatBan/changes", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create a ChatBan (RFC 8620 §5.3 / ChatBan/set create).
    ///
    /// When `input.client_id` is `None`, a ULID is generated automatically.
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
    ///   the matching error list on [`Self::chat_ban_get`]. /set
    ///   create errors (e.g. `invalidProperties`, `forbidden`) appear
    ///   in [`SetResponse::not_created`] rather than as [`Err`].
    pub async fn chat_ban_create(
        &self,
        input: &ChatBanCreateInput<'_>,
        if_in_state: Option<&State>,
    ) -> Result<SetResponse<PartialObject>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut create_obj = serde_json::json!({
            "chatId": input.chat_id,
            "userId": input.user_id,
        });
        if let Some(r) = input.reason {
            create_obj["reason"] = r.into();
        }
        if let Some(ea) = input.expires_at {
            create_obj["expiresAt"] = ea.as_ref().into();
        }
        let client_id = super::resolve_client_id(input.client_id);
        let mut args = serde_json::json!({
            "accountId": account_id,
            "create": { client_id: create_obj },
        });
        if let Some(s) = if_in_state {
            args["ifInState"] = s.as_ref().into();
        }
        let req = super::build_request("ChatBan/set", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Update a ChatBan (RFC 8620 §5.3 / ChatBan/set update).
    ///
    /// Fields of `patch` left at `Patch::Keep` are omitted from the patch
    /// and left unchanged server-side. Requires the `"admin"` role in the
    /// ban's Chat (server-enforced).
    ///
    /// Pass `if_in_state: Some(&state)` to use an optimistic-concurrency guard
    /// (RFC 8620 §5.3 `ifInState`): the server rejects the whole call with
    /// `stateMismatch` unless its current state matches.
    ///
    /// # Errors
    ///
    /// - [`ClientError::Serialize`](jmap_base_client::ClientError::Serialize)
    ///   if serializing a caller-supplied value fails.
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:chat`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::chat_ban_get`]. /set
    ///   update errors (e.g. `forbidden`) appear in
    ///   [`SetResponse::not_updated`] rather than as [`Err`].
    pub async fn chat_ban_update(
        &self,
        id: &Id,
        patch: &ChatBanPatch<'_>,
        if_in_state: Option<&State>,
    ) -> Result<SetResponse<PartialObject>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut patch_map = serde_json::Map::new();
        if let Some(entry) = patch.reason.map_entry()? {
            patch_map.insert("reason".into(), entry);
        }
        if let Some(entry) = patch.expires_at.map_entry()? {
            patch_map.insert("expiresAt".into(), entry);
        }
        // PatchObject is #[serde(transparent)]; see `presence_status_update`.
        let patch_value = serde_json::Value::Object(PatchObject::from_map(patch_map).into_inner());
        let mut args = serde_json::json!({
            "accountId": account_id,
            "update": { id.as_ref(): patch_value },
        });
        if let Some(s) = if_in_state {
            args["ifInState"] = s.as_ref().into();
        }
        let req = super::build_request("ChatBan/set", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Destroy ChatBan objects (RFC 8620 §5.3 / ChatBan/set destroy).
    ///
    /// `ids` must be non-empty; the guard fires before any network call.
    ///
    /// Pass `if_in_state: Some(&state)` to use an optimistic-concurrency guard
    /// (RFC 8620 §5.3 `ifInState`): the server rejects the whole call with
    /// `stateMismatch` unless its current state matches.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `ids` is empty (caller-precondition guard).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:chat`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::chat_ban_get`]. /set
    ///   destroy errors appear in [`SetResponse::not_destroyed`]
    ///   rather than as [`Err`].
    pub async fn chat_ban_destroy(
        &self,
        ids: &[Id],
        if_in_state: Option<&State>,
    ) -> Result<SetResponse<PartialObject>, jmap_base_client::ClientError> {
        if ids.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "chat_ban_destroy: ids may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "destroy": ids,
        });
        if let Some(s) = if_in_state {
            args["ifInState"] = s.as_ref().into();
        }
        let req = super::build_request("ChatBan/set", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}
