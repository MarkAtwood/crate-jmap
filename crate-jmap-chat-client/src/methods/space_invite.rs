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
            args["ids"] = serde_json::to_value(id_slice).expect("Id slice Serialize is infallible");
        }
        if let Some(props) = properties {
            args["properties"] =
                serde_json::to_value(props).expect("&[&str] Serialize is infallible");
        }
        let req = super::build_request("SpaceInvite/get", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to SpaceInvite objects since `since_state` (RFC 8620 §5.2 / SpaceInvite/changes).
    ///
    /// If `has_more_changes` is true in the response, call again with `new_state`
    /// as `since_state` until the flag is false.
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
