//! JMAP Chat — SpaceBan/* method implementations on SessionClient.
//!
//! Spec: JMAP Chat draft §4.18 (SpaceBan/get, /changes, /set).

use jmap_types::{Id, State};

use super::{ChangesResponse, GetResponse, SetResponse, SpaceBanCreateInput};

impl super::SessionClient {
    /// Fetch SpaceBan objects by IDs (JMAP Chat §4.18 SpaceBan/get).
    ///
    /// If `ids` is `None`, returns all SpaceBan objects for the account.
    /// Pass `properties: None` to return all fields.
    pub async fn space_ban_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_chat_types::SpaceBan>, jmap_base_client::ClientError> {
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
        let req = super::build_request("SpaceBan/get", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to SpaceBan objects since `since_state` (RFC 8620 §5.2 / SpaceBan/changes).
    ///
    /// Only members with `"ban"` permission in the Space see all changes;
    /// other members see changes to their own bans only.
    pub async fn space_ban_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `chat_changes`.
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "space_ban_changes: since_state may not be empty".into(),
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
        let req = super::build_request("SpaceBan/changes", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create a SpaceBan (RFC 8620 §5.3 / SpaceBan/set create).
    ///
    /// When `input.client_id` is `None`, a ULID is generated automatically.
    pub async fn space_ban_create(
        &self,
        input: &SpaceBanCreateInput<'_>,
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut create_obj = serde_json::json!({
            "spaceId": input.space_id,
            "userId": input.user_id,
        });
        if let Some(r) = input.reason {
            create_obj["reason"] = r.into();
        }
        if let Some(ea) = input.expires_at {
            create_obj["expiresAt"] = ea.as_ref().into();
        }
        let client_id = super::resolve_client_id(input.client_id);
        let args = serde_json::json!({
            "accountId": account_id,
            "create": { client_id: create_obj },
        });
        let req = super::build_request("SpaceBan/set", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Destroy SpaceBan objects (RFC 8620 §5.3 / SpaceBan/set destroy).
    ///
    /// `ids` must be non-empty; the guard fires before any network call.
    pub async fn space_ban_destroy(
        &self,
        ids: &[Id],
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        if ids.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "space_ban_destroy: ids may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let args = serde_json::json!({
            "accountId": account_id,
            "destroy": ids,
        });
        let req = super::build_request("SpaceBan/set", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}
