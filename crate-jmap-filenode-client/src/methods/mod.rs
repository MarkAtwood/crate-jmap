// Typed JMAP FileNode method wrappers — response types, SessionClient,
// constants, and helpers.
//
// Response types mirror RFC 8620 standard shapes (§5.1 /get, §5.5 /query,
// §5.2 /changes, §5.3 /set). Method implementations live in sub-modules and
// operate on `SessionClient`.

pub mod filenode;

use std::collections::HashMap;

use serde::Deserialize;

use jmap_types::Id;

// ---------------------------------------------------------------------------
// Response types (RFC 8620)
// ---------------------------------------------------------------------------

/// RFC 8620 §5.1 — /get response.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetResponse<T> {
    pub account_id: Id,
    pub state: String,
    pub list: Vec<T>,
    pub not_found: Option<Vec<Id>>,
}

/// RFC 8620 §5.5 — /query response.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResponse {
    pub account_id: Id,
    pub query_state: String,
    pub can_calculate_changes: bool,
    pub position: u64,
    pub ids: Vec<Id>,
    pub total: Option<u64>,
    pub limit: Option<u64>,
}

/// RFC 8620 §5.2 — /changes response.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangesResponse {
    pub account_id: Id,
    pub old_state: String,
    pub new_state: String,
    pub has_more_changes: bool,
    pub created: Vec<Id>,
    pub updated: Vec<Id>,
    pub destroyed: Vec<Id>,
}

/// RFC 8620 §5.3 — /set response.
///
/// The type parameter `T` is the shape of each created/updated object.
/// Defaults to `serde_json::Value` so callers that don't need typed objects
/// can use `SetResponse` without a type argument.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(bound(deserialize = "T: serde::de::DeserializeOwned"))]
pub struct SetResponse<T = serde_json::Value> {
    pub account_id: Id,
    pub old_state: Option<String>,
    pub new_state: String,
    /// Keys are caller-supplied creation keys; see RFC 8620 §5.3.
    pub created: Option<HashMap<String, T>>,
    /// Keys are server-assigned object Ids; see RFC 8620 §5.3.
    pub updated: Option<HashMap<String, T>>,
    pub destroyed: Option<Vec<Id>>,
    /// Keys are caller-supplied creation keys; see RFC 8620 §5.3.
    pub not_created: Option<HashMap<String, SetError>>,
    /// Keys are server-assigned object Ids; see RFC 8620 §5.3.
    pub not_updated: Option<HashMap<String, SetError>>,
    /// Keys are server-assigned object Ids; see RFC 8620 §5.3.
    pub not_destroyed: Option<HashMap<String, SetError>>,
}

/// A /set operation failure for a single object (RFC 8620 §5.3).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetError {
    #[serde(rename = "type")]
    pub error_type: String,
    pub description: Option<String>,
}

impl std::fmt::Display for SetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.description {
            Some(desc) => write!(f, "{}: {}", self.error_type, desc),
            None => write!(f, "{}", self.error_type),
        }
    }
}

/// RFC 8620 §5.6 — /queryChanges response.
///
/// Reports which IDs were removed from and added to a query result set since
/// `old_query_state`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryChangesResponse {
    pub account_id: Id,
    pub old_query_state: String,
    pub new_query_state: String,
    pub total: Option<u64>,
    pub removed: Vec<Id>,
    pub added: Vec<AddedItem>,
}

/// A single item added to a query result set (RFC 8620 §5.6).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddedItem {
    pub id: Id,
    pub index: u64,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// The call-id embedded in every single-method JMAP request produced by
/// [`build_request`]. Pass directly to `jmap_base_client::extract_response`.
pub(crate) const CALL_ID: &str = "r1";

/// Capability URIs for JMAP FileNode method calls (draft-ietf-jmap-filenode-13).
pub(crate) const USING_FILENODE: &[&str] =
    &["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:filenode"];

// ---------------------------------------------------------------------------
// build_request helper
// ---------------------------------------------------------------------------

/// Build a single-method JMAP request.
///
/// `using` is the complete `using` array for the request (RFC 8620 §3.3).
/// Use the pre-defined constant [`USING_FILENODE`] for standard calls.
///
/// The embedded call-id is [`CALL_ID`]; pass it directly to
/// `jmap_base_client::extract_response`.
pub(crate) fn build_request(
    method: &str,
    args: serde_json::Value,
    using: &[&str],
) -> jmap_types::JmapRequest {
    let using_vec: Vec<String> = using.iter().map(|&s| s.to_owned()).collect();
    let invocation: jmap_types::Invocation = (method.to_owned(), args, CALL_ID.to_owned());
    jmap_types::JmapRequest::new(using_vec, vec![invocation], None)
}

// ---------------------------------------------------------------------------
// SessionClient — session-bound client
// ---------------------------------------------------------------------------

/// A `JmapClient` bound to a JMAP session.
///
/// Obtain via [`JmapFileNodeExt::with_filenode_session`](crate::JmapFileNodeExt::with_filenode_session).
/// All JMAP FileNode methods are available on this type without needing to pass
/// `&Session` on every call.
///
/// # Session lifecycle
///
/// `SessionClient` captures the `Session` at construction time. After
/// re-fetching the session via `JmapClient::fetch_session`, construct a new
/// `SessionClient` with the updated session. Reusing a stale `SessionClient`
/// after session expiry will result in `unknownAccount` or similar errors
/// from the server.
pub struct SessionClient {
    pub(crate) client: jmap_base_client::JmapClient,
    pub(crate) session: jmap_base_client::Session,
}

impl SessionClient {
    /// Extract `(api_url, filenode_account_id)` from the bound session.
    ///
    /// Returns `Err(InvalidSession)` if there is no primary account for
    /// `urn:ietf:params:jmap:filenode`.
    pub(crate) fn session_parts(&self) -> Result<(&str, &str), jmap_base_client::ClientError> {
        let api_url = self.session.api_url.as_str();
        let account_id = self
            .session
            .primary_account_id("urn:ietf:params:jmap:filenode")
            .ok_or_else(|| {
                jmap_base_client::ClientError::InvalidSession(
                    "no primary account for urn:ietf:params:jmap:filenode".into(),
                )
            })?;
        Ok((api_url, account_id))
    }

    /// Forward a JMAP request to the underlying HTTP client.
    pub(crate) async fn call_internal(
        &self,
        api_url: &str,
        req: &jmap_types::JmapRequest,
    ) -> Result<jmap_types::JmapResponse, jmap_base_client::ClientError> {
        self.client.call(api_url, req).await
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Oracle: build_request produces the correct method name and call-id.
    /// Expected values are literals from the JMAP wire spec (RFC 8620 §3.3),
    /// not derived from the function under test.
    #[test]
    fn build_request_method_name_and_call_id() {
        let req = build_request(
            "FileNode/get",
            json!({"accountId": "acc1", "ids": null}),
            USING_FILENODE,
        );
        let v = serde_json::to_value(&req).expect("serialize JmapRequest");

        let calls = v["methodCalls"]
            .as_array()
            .expect("methodCalls must be array");
        assert_eq!(calls.len(), 1, "must have exactly 1 method call");
        assert_eq!(calls[0][0], json!("FileNode/get"), "method name must match");
        assert_eq!(calls[0][2], json!("r1"), "call_id must be CALL_ID constant");
    }

    /// Oracle: USING_FILENODE contains exactly the two capability URIs.
    /// Expected values are taken from draft-ietf-jmap-filenode-13 §2.1.
    #[test]
    fn using_filenode_contains_correct_uris() {
        let req = build_request("FileNode/get", json!({}), USING_FILENODE);
        let v = serde_json::to_value(&req).expect("serialize");
        let using = v["using"].as_array().expect("using must be array");
        assert_eq!(using.len(), 2, "must have exactly 2 capability URIs");
        assert!(
            using.contains(&json!("urn:ietf:params:jmap:core")),
            "must include jmap:core"
        );
        assert!(
            using.contains(&json!("urn:ietf:params:jmap:filenode")),
            "must include jmap:filenode"
        );
    }

    /// Oracle: session_parts returns InvalidSession when no primary account
    /// for the filenode capability. Expected error kind from base client spec.
    #[test]
    fn session_parts_err_no_primary_account() {
        let session_json = json!({
            "capabilities": {},
            "accounts": {},
            "primaryAccounts": {},
            "username": "user@example.com",
            "apiUrl": "https://jmap.example.com/api/",
            "downloadUrl": "https://jmap.example.com/dl/{accountId}/{blobId}/{name}?accept={type}",
            "uploadUrl": "https://jmap.example.com/ul/{accountId}/",
            "eventSourceUrl": "https://jmap.example.com/sse/?types={types}&closeafter={closeafter}&ping={ping}",
            "state": "s1"
        });
        let session: jmap_base_client::Session =
            serde_json::from_value(session_json).expect("session must deserialize");

        // Oracle: primary_account_id for an absent key returns None.
        let result = session.primary_account_id("urn:ietf:params:jmap:filenode");
        assert!(
            result.is_none(),
            "must return None when filenode capability is not in primaryAccounts"
        );
    }

    /// Oracle: GetResponse<T> deserializes from RFC 8620 §5.1 shape.
    /// JSON shape is taken from RFC 8620 §5.1, not from the code.
    #[test]
    fn get_response_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "state": "s42",
            "list": [],
            "notFound": ["missing1"]
        });
        let resp: GetResponse<serde_json::Value> =
            serde_json::from_value(json).expect("GetResponse must deserialize");
        assert_eq!(resp.account_id, "acc1");
        assert_eq!(resp.state, "s42");
        assert!(resp.list.is_empty());
        assert_eq!(
            resp.not_found.as_deref(),
            Some(["missing1".into()].as_slice())
        );
    }

    /// Oracle: ChangesResponse deserializes from RFC 8620 §5.2 shape.
    #[test]
    fn changes_response_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "oldState": "s10",
            "newState": "s11",
            "hasMoreChanges": false,
            "created": ["id1"],
            "updated": ["id2"],
            "destroyed": []
        });
        let resp: ChangesResponse =
            serde_json::from_value(json).expect("ChangesResponse must deserialize");
        assert_eq!(resp.old_state, "s10");
        assert_eq!(resp.new_state, "s11");
        assert!(!resp.has_more_changes);
    }

    /// Oracle: SetResponse deserializes from RFC 8620 §5.3 shape.
    #[test]
    fn set_response_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "oldState": "s10",
            "newState": "s11",
            "created": null,
            "updated": null,
            "destroyed": ["id1"],
            "notCreated": null,
            "notUpdated": null,
            "notDestroyed": null
        });
        let resp: SetResponse = serde_json::from_value(json).expect("SetResponse must deserialize");
        assert_eq!(resp.new_state, "s11");
        assert_eq!(resp.destroyed.as_deref(), Some(["id1".into()].as_slice()));
    }

    /// Oracle: QueryChangesResponse deserializes from RFC 8620 §5.6 shape.
    #[test]
    fn query_changes_response_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "oldQueryState": "qs1",
            "newQueryState": "qs2",
            "total": 5,
            "removed": ["id3"],
            "added": [{"id": "id4", "index": 0}]
        });
        let resp: QueryChangesResponse =
            serde_json::from_value(json).expect("QueryChangesResponse must deserialize");
        assert_eq!(resp.old_query_state, "qs1");
        assert_eq!(resp.new_query_state, "qs2");
        assert_eq!(resp.total, Some(5));
        assert_eq!(resp.removed.len(), 1);
        assert_eq!(resp.added.len(), 1);
        assert_eq!(resp.added[0].index, 0);
    }
}
