//! Space root discovery — urn:ietf:params:jmap:chat:filenode
//!
//! Locates the Directories that root a Space's file tree. Only call when
//! `ChatSessionExt::chat_filenode_capability` returns `Some`.
//!
//! Spec: draft-atwood-jmap-chat-filenode-00 §Discovering Space Roots

use jmap_types::Id;

use super::QueryResponse;

impl super::SessionClient {
    /// Find every Space root of `space_id`.
    ///
    /// Sends `FileNode/query` filtered on `spaceId: <space_id>` against the
    /// primary JMAP Chat account and returns the ids in the order the
    /// server gave them. A Space has "zero, one, or more Space roots"
    /// (draft-atwood-jmap-chat-filenode-00 §Space File Tree), so an empty
    /// result means the Space has no file tree yet; the caller MAY then
    /// create a root via `FileNode/set`, which needs `"manage_space"`.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:chat`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call); servers
    ///   that do not advertise `urn:ietf:params:jmap:filenode` or
    ///   `urn:ietf:params:jmap:chat:filenode` return `unknownCapability`
    ///   as a [`MethodError`](jmap_base_client::ClientError::MethodError).
    pub async fn space_roots(
        &self,
        space_id: &Id,
    ) -> Result<Vec<Id>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let args = serde_json::json!({
            "accountId": account_id,
            "filter": { "spaceId": space_id },
        });
        let req = super::build_request("FileNode/query", args, super::USING_CHAT_FILENODE);
        let resp = self.call_internal(api_url, &req).await?;
        let query: QueryResponse = jmap_base_client::extract_response(&resp, super::CALL_ID)?;
        Ok(query.ids)
    }
}
