//! JMAP Chat — SpaceInvite/* method implementations on SessionClient.
//!
//! Spec: JMAP Chat draft §4.17 (SpaceInvite/get, /changes, /set).

use jmap_types::{Id, State};

use super::{ChangesResponse, GetResponse, SetResponse, SpaceInviteCreateInput};

impl super::SessionClient {
    /// Fetch SpaceInvite objects by IDs (JMAP Chat §4.17 SpaceInvite/get).
    ///
    /// If `ids` is `None`, returns all SpaceInvite objects for the account.
    /// Pass `properties: None` to return all fields.
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
    pub async fn space_invite_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_chat_types::SpaceInvite>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        // Omit `ids` / `properties` when None — see the matching comment on
        // `chat_get` for the rationale (consistent with set/changes/query).
        let mut args = serde_json::json!({ "accountId": account_id });
        if let Some(id_slice) = ids {
            args["ids"] =
                serde_json::to_value(id_slice).map_err(jmap_base_client::ClientError::Parse)?;
        }
        if let Some(props) = properties {
            args["properties"] =
                serde_json::to_value(props).map_err(jmap_base_client::ClientError::Parse)?;
        }
        let req = super::build_request("SpaceInvite/get", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to SpaceInvite objects since `since_state` (RFC 8620 §5.2 / SpaceInvite/changes).
    ///
    /// If `has_more_changes` is true in the response, call again with `new_state`
    /// as `since_state` until the flag is false.
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
    ///   the matching error list on [`Self::space_invite_get`].
    pub async fn space_invite_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `chat_changes`.
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "space_invite_changes: since_state may not be empty".into(),
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
        let req = super::build_request("SpaceInvite/changes", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create a SpaceInvite (RFC 8620 §5.3 / SpaceInvite/set create).
    ///
    /// When `input.client_id` is `None`, a ULID is generated automatically.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:chat`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::space_invite_get`]. /set
    ///   create errors (e.g. `invalidProperties`, `forbidden`) appear
    ///   in [`SetResponse::not_created`] rather than as [`Err`].
    pub async fn space_invite_create(
        &self,
        input: &SpaceInviteCreateInput<'_>,
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut obj = serde_json::json!({ "spaceId": input.space_id });
        if let Some(ch) = input.default_channel_id {
            obj["defaultChannelId"] = ch.as_ref().into();
        }
        if let Some(ea) = input.expires_at {
            obj["expiresAt"] = ea.as_ref().into();
        }
        if let Some(mu) = input.max_uses {
            obj["maxUses"] = mu.into();
        }
        let client_id = super::resolve_client_id(input.client_id);
        let args = serde_json::json!({
            "accountId": account_id,
            "create": { client_id: obj },
        });
        let req = super::build_request("SpaceInvite/set", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Destroy SpaceInvite objects (RFC 8620 §5.3 / SpaceInvite/set destroy).
    ///
    /// `ids` must be non-empty; the guard fires before any network call.
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
    ///   the matching error list on [`Self::space_invite_get`]. /set
    ///   destroy errors appear in [`SetResponse::not_destroyed`]
    ///   rather than as [`Err`].
    pub async fn space_invite_destroy(
        &self,
        ids: &[Id],
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        if ids.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "space_invite_destroy: ids may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let args = serde_json::json!({
            "accountId": account_id,
            "destroy": ids,
        });
        let req = super::build_request("SpaceInvite/set", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}
