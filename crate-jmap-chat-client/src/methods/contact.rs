use super::{
    ChangesResponse, ChatContactPatch, ChatContactQueryInput, GetResponse, QueryChangesResponse,
    QueryResponse, SetResponse,
};

impl super::SessionClient {
    /// Fetch ChatContact objects by IDs (JMAP Chat §5 ChatContact/get).
    ///
    /// If `ids` is `None`, returns all ChatContacts for the account.
    pub async fn chat_contact_get(
        &self,
        ids: Option<&[&str]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_chat_types::ChatContact>, jmap_base_client::ClientError> {
        if let Some(id_slice) = ids {
            for id in id_slice.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "chat_contact_get: ids element may not be empty".into(),
                    ));
                }
            }
        }
        let (api_url, account_id) = self.session_parts()?;
        let args = serde_json::json!({
            "accountId": account_id,
            "ids": ids,
            "properties": properties,
        });
        let req = super::build_request("ChatContact/get", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to ChatContact objects since `since_state` (RFC 8620 §5.2).
    pub async fn chat_contact_changes(
        &self,
        since_state: &str,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        if since_state.is_empty() {
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

    /// Update ChatContact properties (JMAP Chat §ChatContact/set).
    ///
    /// Supports `blocked` (Boolean) and `displayName` (nullable String).
    /// Create and destroy are not supported by spec; the server returns `forbidden`.
    pub async fn chat_contact_update(
        &self,
        id: &str,
        patch: &ChatContactPatch<'_>,
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        if id.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "chat_contact_update: id may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut patch_map = serde_json::Map::new();
        if let Some(b) = patch.blocked {
            patch_map.insert("blocked".into(), b.into());
        }
        if let Some(entry) = patch
            .display_name
            .map_entry()
            .map_err(jmap_base_client::ClientError::Parse)?
        {
            patch_map.insert("displayName".into(), entry);
        }
        let args = serde_json::json!({
            "accountId": account_id,
            "update": { id: serde_json::Value::Object(patch_map) },
        });
        let req = super::build_request("ChatContact/set", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Query ChatContact IDs with optional filter (JMAP Chat §ChatContact/query).
    ///
    /// Supported filter keys: `blocked`, `presence`. Supported sort properties:
    /// `"lastSeenAt"`, `"login"`, `"lastActiveAt"`.
    pub async fn chat_contact_query(
        &self,
        input: &ChatContactQueryInput,
    ) -> Result<QueryResponse, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut filter = serde_json::Map::new();
        if let Some(b) = input.filter_blocked {
            filter.insert("blocked".into(), b.into());
        }
        if let Some(p) = &input.filter_presence {
            filter.insert(
                "presence".into(),
                serde_json::to_value(p).map_err(jmap_base_client::ClientError::Parse)?,
            );
        }
        let filter_val = if filter.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::Value::Object(filter)
        };
        let mut args = serde_json::json!({
            "accountId": account_id,
            "filter": filter_val,
        });
        if let Some(sp) = &input.sort_property {
            let property =
                serde_json::to_value(sp).map_err(jmap_base_client::ClientError::Parse)?;
            args["sort"] = serde_json::json!([{
                "property": property,
                "isAscending": input.sort_ascending.unwrap_or(false),
            }]);
        }
        if let Some(p) = input.position {
            args["position"] = p.into();
        }
        if let Some(l) = input.limit {
            args["limit"] = l.into();
        }
        let req = super::build_request("ChatContact/query", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch query-result changes for ChatContact since `since_query_state`
    /// (RFC 8620 §5.6 / ChatContact/queryChanges).
    pub async fn chat_contact_query_changes(
        &self,
        since_query_state: &str,
        max_changes: Option<u64>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        if since_query_state.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "chat_contact_query_changes: since_query_state may not be empty".into(),
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
        let req = super::build_request("ChatContact/queryChanges", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}
