// JMAP FileNode — FileNode/* method implementations on SessionClient.
//
// Each method follows the standard five-step pattern:
//   1. Validate arguments (empty-string guards).
//   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//   3. Build args JSON with `serde_json::json!({…})`.
//   4. Call `build_request(method_name, args, USING_FILENODE)`.
//   5. Call `self.call_internal(api_url, &req).await?`.
//   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.

use std::collections::HashMap;

use super::{ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SetResponse};
use jmap_types::{Id, PatchObject, State};

// ---------------------------------------------------------------------------
// FileNode-specific input types
// ---------------------------------------------------------------------------

/// The action to take when a FileNode with the same name already exists
/// (draft-ietf-jmap-filenode-13 §3.2.3 `onExists` argument).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FileNodeOnExists {
    /// Replace the existing node with the new one.
    Replace,
    /// Rename the new node to avoid the collision.
    Rename,
}

/// Parameters for FileNode/set requests
/// (draft-ietf-jmap-filenode-13 §3.2.3).
///
/// All fields are optional; omitted fields take server defaults.
#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileNodeSetParams {
    /// If true, destroying a directory also destroys all its descendants.
    /// Server default: false (destroy fails if children exist).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_destroy_remove_children: Option<bool>,
    /// How to handle a name collision with an existing sibling node.
    /// `None` means the server's default policy applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_exists: Option<FileNodeOnExists>,
    /// If true, name comparisons are case-insensitive when checking for
    /// sibling name collisions. Server default: false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compare_case_insensitively: Option<bool>,
}

/// Parameters for FileNode/copy requests
/// (draft-ietf-jmap-filenode-13 §3.2.4).
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileNodeCopyParams {
    /// The account that is the source of the copy operation.
    pub from_account_id: Id,
}

// ---------------------------------------------------------------------------
// Method implementations
// ---------------------------------------------------------------------------

impl super::SessionClient {
    /// Fetch FileNode objects by IDs (draft-ietf-jmap-filenode-13 §3.2.1).
    ///
    /// If `ids` is `None`, the server returns all FileNodes for the account.
    /// Pass `properties: None` to return all fields.
    /// If `fetch_parents` is `Some(true)`, the server also returns all ancestor
    /// nodes of the requested IDs.
    pub async fn file_node_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
        fetch_parents: Option<bool>,
    ) -> Result<GetResponse<jmap_filenode_types::FileNode>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        // Omit `ids` / `properties` entirely when None rather than sending
        // an explicit JSON null. RFC 8620 §5.1 accepts both shapes, but the
        // crate's other builders (set/changes/query) consistently use the
        // conditional-add idiom; matching it here keeps the wire request
        // canonical and avoids "present-but-null vs absent" interop quirks
        // in proxies / audit loggers.
        let mut args = serde_json::json!({ "accountId": account_id });
        if let Some(id_slice) = ids {
            args["ids"] = serde_json::to_value(id_slice).expect("Id slice Serialize is infallible");
        }
        if let Some(props) = properties {
            args["properties"] = serde_json::Value::Array(
                props.iter().copied().map(serde_json::Value::from).collect(),
            );
        }
        if let Some(fp) = fetch_parents {
            args["fetchParents"] = fp.into();
        }
        let req = super::build_request("FileNode/get", args, super::USING_FILENODE);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to FileNode objects since `since_state`
    /// (draft-ietf-jmap-filenode-13 §3.2.2).
    ///
    /// If `has_more_changes` is true in the response, call again with
    /// `new_state` as `since_state` until the flag is false.
    pub async fn file_node_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: an empty State token has no meaning on the wire
        // even though State is a typed newtype.
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "file_node_changes: since_state may not be empty".into(),
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
        let req = super::build_request("FileNode/changes", args, super::USING_FILENODE);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create, update, or destroy FileNode objects
    /// (draft-ietf-jmap-filenode-13 §3.2.3).
    ///
    /// Pass `create`, `update`, and/or `destroy` as needed. All three are
    /// optional; pass `None` to omit any operation from the request.
    ///
    /// `update` is `Option<HashMap<Id, PatchObject>>` (RFC 8620 §5.3). Wire
    /// format is unchanged from a plain JSON object because [`PatchObject`]
    /// is `#[serde(transparent)]`; the typed parameter binds the JSON Pointer
    /// key + null-leaf removal contract to the type system.
    ///
    /// Use `params` to set FileNode-specific top-level arguments
    /// (`onDestroyRemoveChildren`, `onExists`, `compareCaseInsensitively`).
    pub async fn file_node_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<HashMap<Id, PatchObject>>,
        destroy: Option<Vec<Id>>,
        params: Option<FileNodeSetParams>,
    ) -> Result<SetResponse<jmap_filenode_types::FileNode>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
        });
        if let Some(c) = create {
            args["create"] = c;
        }
        if let Some(u) = update {
            args["update"] = serde_json::to_value(&u).map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "file_node_set: serializing update map failed: {e}"
                ))
            })?;
        }
        if let Some(d) = destroy {
            args["destroy"] = serde_json::to_value(&d).expect("Id Vec Serialize is infallible");
        }
        if let Some(p) = params {
            let params_val = serde_json::to_value(&p).map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "file_node_set: failed to serialize params: {e}"
                ))
            })?;
            if let serde_json::Value::Object(map) = params_val {
                for (k, v) in map {
                    args[k] = v;
                }
            }
        }
        let req = super::build_request("FileNode/set", args, super::USING_FILENODE);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Copy FileNodes from another account
    /// (draft-ietf-jmap-filenode-13 §3.2.4).
    ///
    /// `from_account_id` is the source account. `create` is a JSON object
    /// mapping creation keys to copy-creation objects. `on_exists` and
    /// `compare_case_insensitively` are optional collision-handling parameters.
    /// Copy FileNode objects from `from_account_id` into this account
    /// (draft-ietf-jmap-filenode-13 §3.2.4 — FileNode/copy).
    ///
    /// Accepts the same optional arguments as `FileNode/set`:
    /// - `on_destroy_remove_children`: when `true`, any children of a destroyed
    ///   source node are also removed (§3.2.4, §3.2.3).
    /// - `on_exists`: collision policy at the destination.
    /// - `compare_case_insensitively`: case-folding for name collisions.
    pub async fn file_node_copy(
        &self,
        from_account_id: &Id,
        create: serde_json::Value,
        on_destroy_remove_children: Option<bool>,
        on_exists: Option<FileNodeOnExists>,
        compare_case_insensitively: Option<bool>,
    ) -> Result<SetResponse<jmap_filenode_types::FileNode>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "fromAccountId": from_account_id,
            "accountId": account_id,
            "create": create,
        });
        if let Some(odrc) = on_destroy_remove_children {
            args["onDestroyRemoveChildren"] = odrc.into();
        }
        if let Some(oe) = on_exists {
            args["onExists"] = serde_json::to_value(&oe).map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "file_node_copy: failed to serialize onExists: {e}"
                ))
            })?;
        }
        if let Some(cci) = compare_case_insensitively {
            args["compareCaseInsensitively"] = cci.into();
        }
        let req = super::build_request("FileNode/copy", args, super::USING_FILENODE);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Query FileNode IDs with optional filter, sort, and pagination
    /// (draft-ietf-jmap-filenode-13 §3.2.5).
    ///
    /// The `depth` parameter controls recursive descent: `None` or `Some(0)`
    /// means no recursion; `Some(n)` recurses `n` levels into subdirectories.
    pub async fn file_node_query(
        &self,
        filter: Option<serde_json::Value>,
        sort: Option<serde_json::Value>,
        position: Option<u64>,
        limit: Option<u64>,
        depth: Option<u64>,
    ) -> Result<QueryResponse, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
        });
        if let Some(f) = filter {
            args["filter"] = f;
        }
        if let Some(s) = sort {
            args["sort"] = s;
        }
        if let Some(p) = position {
            args["position"] = p.into();
        }
        if let Some(l) = limit {
            args["limit"] = l.into();
        }
        if let Some(d) = depth {
            args["depth"] = d.into();
        }
        let req = super::build_request("FileNode/query", args, super::USING_FILENODE);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch query-result changes for FileNode since `since_query_state`
    /// (draft-ietf-jmap-filenode-13 §3.2.6).
    ///
    /// Returns which FileNode IDs were removed from or added to the query
    /// result set since the given state. `max_changes` may be `None`.
    pub async fn file_node_query_changes(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see file_node_changes.
        if since_query_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "file_node_query_changes: since_query_state may not be empty".into(),
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
        let req = super::build_request("FileNode/queryChanges", args, super::USING_FILENODE);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::{build_request, QueryResponse, SetResponse, CALL_ID, USING_FILENODE};
    use super::{FileNodeOnExists, FileNodeSetParams};
    use serde_json::json;

    // ── Empty-string guards ─────────────────────────────────────────────────

    // The `file_node_get_empty_id_guard` inline smoke test was removed by the
    // JMAP-6by7.6 typed-Id refactor. It was vacuous because it only iterated
    // a local `&[""]` slice and asserted `is_empty()` found the empty value,
    // without invoking any production method. Under typed `&[Id]` parameters,
    // an empty-Id input is impossible to express through the API
    // (`Id::new_validated("")` returns `Err` at the call site) so the bug it
    // pretended to test is unrepresentable.

    // The InvalidArgument guards for empty since_state, since_query_state,
    // destroy IDs, and from_account_id live in the FileNode production code;
    // testing them requires a wiremock-backed async harness.
    // See JMAP-sc1b.64.

    // ── Serialization oracles ───────────────────────────────────────────────

    /// Oracle: FileNodeOnExists::Replace serializes to "replace".
    /// Expected value taken directly from draft-ietf-jmap-filenode-13 §3.2.3.
    #[test]
    fn file_node_on_exists_replace_serializes() {
        let val = serde_json::to_value(FileNodeOnExists::Replace).expect("serialize");
        assert_eq!(
            val,
            json!("replace"),
            "Replace must serialize to \"replace\""
        );
    }

    /// Oracle: FileNodeOnExists::Rename serializes to "rename".
    /// Expected value taken directly from draft-ietf-jmap-filenode-13 §3.2.3.
    #[test]
    fn file_node_on_exists_rename_serializes() {
        let val = serde_json::to_value(FileNodeOnExists::Rename).expect("serialize");
        assert_eq!(val, json!("rename"), "Rename must serialize to \"rename\"");
    }

    /// Oracle: FileNodeSetParams with onDestroyRemoveChildren=true serializes correctly.
    /// Expected field name "onDestroyRemoveChildren" from draft-ietf-jmap-filenode-13 §3.2.3.
    #[test]
    fn file_node_set_params_on_destroy_serializes() {
        let params = FileNodeSetParams {
            on_destroy_remove_children: Some(true),
            on_exists: None,
            compare_case_insensitively: None,
        };
        let val = serde_json::to_value(&params).expect("serialize");
        assert_eq!(
            val["onDestroyRemoveChildren"],
            json!(true),
            "onDestroyRemoveChildren must be true"
        );
        assert!(
            val.get("onExists").is_none(),
            "onExists must be absent when None"
        );
    }

    /// Oracle: FileNodeSetParams with all fields set serializes all three.
    #[test]
    fn file_node_set_params_all_fields_serializes() {
        let params = FileNodeSetParams {
            on_destroy_remove_children: Some(false),
            on_exists: Some(FileNodeOnExists::Replace),
            compare_case_insensitively: Some(true),
        };
        let val = serde_json::to_value(&params).expect("serialize");
        assert_eq!(val["onDestroyRemoveChildren"], json!(false));
        assert_eq!(val["onExists"], json!("replace"));
        assert_eq!(val["compareCaseInsensitively"], json!(true));
    }

    // ── Request shape oracles ───────────────────────────────────────────────

    /// Oracle: FileNode/get request has correct method name and using array.
    /// Expected JSON shape from RFC 8620 §3.3.
    #[test]
    fn file_node_get_request_shape() {
        let args = json!({
            "accountId": "acc1",
            "ids": null,
            "properties": null,
        });
        let req = build_request("FileNode/get", args, USING_FILENODE);
        let v = serde_json::to_value(&req).expect("serialize");

        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("FileNode/get"), "method name");
        assert_eq!(calls[0][2], json!(CALL_ID), "call id");

        let using = v["using"].as_array().expect("using");
        assert!(using.contains(&json!("urn:ietf:params:jmap:filenode")));
    }

    /// Oracle: FileNode/query with depth includes depth in args JSON.
    /// Expected: args object has "depth" key with the provided numeric value.
    /// Source: draft-ietf-jmap-filenode-13 §3.2.5.
    #[test]
    fn file_node_query_with_depth_includes_depth_in_args() {
        let mut args = json!({ "accountId": "acc1" });
        args["depth"] = 2u64.into();

        let req = build_request("FileNode/query", args, USING_FILENODE);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(
            calls[0][1]["depth"],
            json!(2),
            "depth must appear in args with value 2"
        );
    }

    /// Oracle: FileNode/query without depth omits depth from args.
    /// Expected: args object lacks "depth" key when not supplied.
    #[test]
    fn file_node_query_without_depth_omits_depth() {
        let args = json!({ "accountId": "acc1" });
        let req = build_request("FileNode/query", args, USING_FILENODE);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert!(
            calls[0][1].get("depth").is_none(),
            "depth must be absent when not supplied"
        );
    }

    /// Oracle: FileNode/changes request includes sinceState in args.
    /// Expected: args object has "sinceState" key with the provided value.
    #[test]
    fn file_node_changes_request_includes_since_state() {
        let args = json!({
            "accountId": "acc1",
            "sinceState": "state42",
        });
        let req = build_request("FileNode/changes", args, USING_FILENODE);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][1]["sinceState"], json!("state42"));
    }

    /// Oracle: FileNode/set with destroy list sends destroy array in args.
    /// Expected: destroy is a JSON array of string IDs.
    #[test]
    fn file_node_set_destroy_request_shape() {
        let destroy_ids = ["id1", "id2"];
        let destroy_val = serde_json::Value::Array(
            destroy_ids
                .iter()
                .copied()
                .map(serde_json::Value::from)
                .collect(),
        );
        let mut args = json!({ "accountId": "acc1" });
        args["destroy"] = destroy_val;

        let req = build_request("FileNode/set", args, USING_FILENODE);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("FileNode/set"));
        let destroy_arr = calls[0][1]["destroy"].as_array().expect("destroy array");
        assert_eq!(destroy_arr.len(), 2);
        assert!(destroy_arr.contains(&json!("id1")));
        assert!(destroy_arr.contains(&json!("id2")));
    }

    /// Oracle: FileNode/copy request includes fromAccountId in args.
    /// Expected: args object has "fromAccountId" key.
    /// Source: draft-ietf-jmap-filenode-13 §3.2.4.
    #[test]
    fn file_node_copy_request_includes_from_account_id() {
        let args = json!({
            "fromAccountId": "source-account",
            "accountId": "dest-account",
            "create": {},
        });
        let req = build_request("FileNode/copy", args, USING_FILENODE);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("FileNode/copy"), "method name");
        assert_eq!(
            calls[0][1]["fromAccountId"],
            json!("source-account"),
            "fromAccountId must be present"
        );
    }

    /// Oracle: FileNode/copy with onDestroyRemoveChildren=true includes the key
    /// in the request (draft-ietf-jmap-filenode-13 §3.2.4).
    #[test]
    fn file_node_copy_with_on_destroy_remove_children_includes_key() {
        let mut args = json!({
            "fromAccountId": "source",
            "accountId": "dest",
            "create": {},
        });
        args["onDestroyRemoveChildren"] = json!(true);
        let req = build_request("FileNode/copy", args, USING_FILENODE);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(
            calls[0][1]["onDestroyRemoveChildren"],
            json!(true),
            "onDestroyRemoveChildren must appear when set to true"
        );
    }

    /// Oracle: FileNode/copy with onDestroyRemoveChildren=None must NOT include
    /// the key in the request.
    #[test]
    fn file_node_copy_without_on_destroy_remove_children_omits_key() {
        let args = json!({
            "fromAccountId": "source",
            "accountId": "dest",
            "create": {},
        });
        let req = build_request("FileNode/copy", args, USING_FILENODE);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert!(
            calls[0][1].get("onDestroyRemoveChildren").is_none(),
            "onDestroyRemoveChildren must be absent when None"
        );
    }

    // ── Response deserialization oracles ────────────────────────────────────

    /// Oracle: QueryResponse deserializes from RFC 8620 §5.5 shape.
    /// JSON shape from RFC 8620 §5.5, not from the code.
    #[test]
    fn query_response_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "queryState": "qs1",
            "canCalculateChanges": true,
            "position": 0,
            "ids": ["id1", "id2"],
            "total": 2,
            "limit": 256
        });
        let resp: QueryResponse =
            serde_json::from_value(json).expect("QueryResponse must deserialize");
        assert_eq!(resp.account_id, "acc1");
        assert_eq!(resp.query_state, "qs1");
        assert!(resp.can_calculate_changes);
        assert_eq!(resp.position, 0);
        assert_eq!(resp.ids.len(), 2);
        assert_eq!(resp.total, Some(2));
        assert_eq!(resp.limit, Some(256));
    }

    /// Oracle: SetResponse<FileNode> deserializes from RFC 8620 §5.3 shape
    /// with a typed FileNode in the created map.
    /// FileNode JSON from draft-ietf-jmap-filenode-13 §3.1 (minimal directory node).
    #[test]
    fn set_response_with_typed_file_node_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "oldState": "s1",
            "newState": "s2",
            "created": {
                "new1": {
                    "id": "node-abc",
                    "parentId": null,
                    "blobId": null,
                    "target": null,
                    "size": null,
                    "name": "Documents",
                    "type": null,
                    "shareWith": null
                }
            },
            "updated": null,
            "destroyed": null,
            "notCreated": null,
            "notUpdated": null,
            "notDestroyed": null
        });
        let resp: SetResponse<jmap_filenode_types::FileNode> =
            serde_json::from_value(json).expect("SetResponse<FileNode> must deserialize");
        let created = resp.created.expect("created must be Some");
        let node = created.get("new1").expect("new1 must be present");
        assert_eq!(node.id, "node-abc");
        assert_eq!(node.name, "Documents");
    }
}
