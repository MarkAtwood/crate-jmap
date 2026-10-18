//! Space file tree discovery — urn:ietf:params:jmap:chat:filenode
//!
//! Locates the FileNode that roots a Space's file tree. Only call when
//! `ChatSessionExt::chat_filenode_capability` returns `Some`.
//!
//! Spec: draft-atwood-jmap-chat-filenode-00 §Discovering the File Tree Root

use jmap_types::Id;

use super::QueryResponse;

impl super::SessionClient {
    /// Find the root FileNode of `space_id`'s file tree.
    ///
    /// Sends `FileNode/query` filtered on
    /// `role: "urn:jmap:chat:filenode:space"` and `spaceId: <space_id>`
    /// against the primary JMAP Chat account. Returns `Ok(None)` when the
    /// Space has no file tree yet; the caller MAY then create one via
    /// `FileNode/set` (which requires `"manage_space"` permission).
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:chat`.
    /// - [`ClientError::UnexpectedResponse`](jmap_base_client::ClientError::UnexpectedResponse)
    ///   if the server returns more than one root; the draft permits "at
    ///   most one root per Space".
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call); servers
    ///   that do not advertise `urn:ietf:params:jmap:filenode` or
    ///   `urn:ietf:params:jmap:chat:filenode` return `unknownCapability`
    ///   as a [`MethodError`](jmap_base_client::ClientError::MethodError).
    pub async fn space_file_tree_root(
        &self,
        space_id: &Id,
    ) -> Result<Option<Id>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let args = serde_json::json!({
            "accountId": account_id,
            "filter": {
                "role": jmap_chat_types::CHAT_FILENODE_SPACE_ROLE,
                "spaceId": space_id,
            },
        });
        let req = super::build_request("FileNode/query", args, super::USING_CHAT_FILENODE);
        let resp = self.call_internal(api_url, &req).await?;
        let query: QueryResponse = jmap_base_client::extract_response(&resp, super::CALL_ID)?;
        let mut ids = query.ids.into_iter();
        let root = ids.next();
        if ids.next().is_some() {
            return Err(jmap_base_client::ClientError::UnexpectedResponse(format!(
                "FileNode/query returned more than one file tree root for Space {space_id}"
            )));
        }
        Ok(root)
    }
}
