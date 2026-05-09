// CustomEmoji method implementations for SessionClient.
//
// Spec: JMAP Chat draft §4.16 (CustomEmoji/get, /changes, /set, /query, /queryChanges)
// Capability: urn:ietf:params:jmap:chat

use jmap_types::{Id, State};

use super::{
    ChangesResponse, CustomEmojiCreateInput, CustomEmojiQueryInput, GetResponse,
    QueryChangesResponse, QueryResponse, SetResponse,
};

impl super::SessionClient {
    /// Fetch CustomEmoji objects by IDs (JMAP Chat §4.16 CustomEmoji/get).
    ///
    /// If `ids` is `None`, returns all CustomEmoji objects visible to the account
    /// (Space-specific and server-global).
    pub async fn custom_emoji_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_chat_types::CustomEmoji>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        // Omit `ids` / `properties` when None — see the matching comment on
        // `chat_get` for the rationale (consistent with set/changes/query).
        let mut args = serde_json::json!({ "accountId": account_id });
        if let Some(id_slice) = ids {
            args["ids"] = serde_json::to_value(id_slice).expect("Id slice Serialize is infallible");
        }
        if let Some(props) = properties {
            args["properties"] = serde_json::Value::Array(
                props.iter().copied().map(serde_json::Value::from).collect(),
            );
        }
        let req = super::build_request("CustomEmoji/get", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to CustomEmoji objects since `since_state`
    /// (RFC 8620 §5.2 / JMAP Chat §4.16 CustomEmoji/changes).
    pub async fn custom_emoji_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `chat_changes`.
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "custom_emoji_changes: since_state may not be empty".into(),
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
        let req = super::build_request("CustomEmoji/changes", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create a CustomEmoji (RFC 8620 §5.3 / JMAP Chat §4.16 CustomEmoji/set create).
    ///
    /// When `input.client_id` is `None`, a ULID is generated automatically.
    pub async fn custom_emoji_create(
        &self,
        input: &CustomEmojiCreateInput<'_>,
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        if input.name.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "custom_emoji_create: name may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut create_obj = serde_json::json!({
            "name": input.name,
            "blobId": input.blob_id,
        });
        if let Some(sid) = input.space_id {
            create_obj["spaceId"] = sid.as_ref().into();
        }
        let client_id = super::resolve_client_id(input.client_id);
        let args = serde_json::json!({
            "accountId": account_id,
            "create": { client_id: create_obj },
        });
        let req = super::build_request("CustomEmoji/set", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Destroy CustomEmoji objects (RFC 8620 §5.3 / JMAP Chat §4.16 CustomEmoji/set destroy).
    ///
    /// `ids` must be non-empty; the guard fires before any network call.
    pub async fn custom_emoji_destroy(
        &self,
        ids: &[Id],
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        if ids.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "custom_emoji_destroy: ids may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let args = serde_json::json!({
            "accountId": account_id,
            "destroy": ids,
        });
        let req = super::build_request("CustomEmoji/set", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Query CustomEmoji IDs (RFC 8620 §5.5 / JMAP Chat §4.16 CustomEmoji/query).
    pub async fn custom_emoji_query(
        &self,
        input: &CustomEmojiQueryInput<'_>,
    ) -> Result<QueryResponse, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
        });
        if let Some(sid) = input.filter_space_id {
            args["filter"] = serde_json::json!({ "spaceId": sid });
        }
        if let Some(p) = input.position {
            args["position"] = p.into();
        }
        if let Some(l) = input.limit {
            args["limit"] = l.into();
        }
        let req = super::build_request("CustomEmoji/query", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch query-result changes for CustomEmoji since `since_query_state`
    /// (RFC 8620 §5.6 / JMAP Chat §4.16 CustomEmoji/queryChanges).
    pub async fn custom_emoji_query_changes(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `chat_changes`.
        if since_query_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "custom_emoji_query_changes: since_query_state may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "sinceQueryState": since_query_state,
        });
        if let Some(mc) = max_changes {
            args["maxChanges"] = mc.into();
        }
        let req = super::build_request("CustomEmoji/queryChanges", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}
