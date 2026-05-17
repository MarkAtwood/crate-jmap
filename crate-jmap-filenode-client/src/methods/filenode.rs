//! JMAP FileNode — FileNode/* method implementations on SessionClient.
//!
//! Each method follows the standard five-step pattern:
//!   1. Validate arguments (empty-string guards).
//!   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//!   3. Build args JSON with `serde_json::json!({…})`.
//!   4. Call `build_request(method_name, args, USING_FILENODE)`.
//!   5. Call `self.call_internal(api_url, &req).await?`.
//!   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.

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
#[derive(Debug, Default, Clone, serde::Serialize)]
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    ///
    /// **Constraint**: keys in `extra` MUST NOT collide with the
    /// typed-field wire names above (the camelCase spelling — e.g.
    /// `"accountId"`, `"ids"`, `"properties"`, `"blobIds"`,
    /// `"fromAccountId"`, etc.). On collision the typed-field value
    /// wins on the wire and the `extra` value is silently dropped at
    /// serialization. Place vendor extensions under vendor-prefixed
    /// keys (e.g. `"acmeCorpFoo"`) to avoid the collision class.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Parameters for FileNode/copy requests
/// (draft-ietf-jmap-filenode-13 §3.2.4).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileNodeCopyParams {
    /// The account that is the source of the copy operation.
    pub from_account_id: Id,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    ///
    /// **Constraint**: keys in `extra` MUST NOT collide with the
    /// typed-field wire names above (the camelCase spelling — e.g.
    /// `"accountId"`, `"ids"`, `"properties"`, `"blobIds"`,
    /// `"fromAccountId"`, etc.). On collision the typed-field value
    /// wins on the wire and the `extra` value is silently dropped at
    /// serialization. Place vendor extensions under vendor-prefixed
    /// keys (e.g. `"acmeCorpFoo"`) to avoid the collision class.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Method implementations
// ---------------------------------------------------------------------------

impl super::SessionClient {
    /// Fetch FileNode objects by IDs (draft-ietf-jmap-filenode-13 §3.2.1).
    ///
    /// If `ids` is `None`, the server returns all FileNodes for the account,
    /// SUBJECT TO the server's `maxObjectsInGet` cap (RFC 8620 §5.1).
    /// For production use, scope the result set via the corresponding
    /// /query method first and pass explicit ids here to avoid
    /// `requestTooLarge` errors when the account holds more objects
    /// than the cap.
    /// Pass `properties: None` to return all fields.
    /// If `fetch_parents` is `Some(true)`, the server also returns all ancestor
    /// nodes of the requested IDs.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:filenode`.
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
            args["properties"] =
                serde_json::to_value(props).expect("&[&str] Serialize is infallible");
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
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `since_state` is the empty string (defence-in-depth —
    ///   `State` constructed via [`State::from`](jmap_types::State::from)
    ///   accepts empty strings, but an empty `sinceState` is never
    ///   useful and would otherwise generate a wasted round-trip).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:filenode`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::file_node_get`].
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
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:filenode`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `serde_json::to_value` fails on the `update` patch map or on
    ///   `params` (pathological conditions only — allocation failure,
    ///   or a vendor value in `params.extra` or a `PatchObject` whose
    ///   JSON tree exceeds `serde_json`'s recursion limit). The
    ///   transient memory peak for very large `update` maps is roughly
    ///   3-4× the `HashMap`'s in-memory size (source map +
    ///   `serde_json::Value` tree + serialized `Vec<u8>` body); callers
    ///   dealing with thousands of patches per call may prefer to batch.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::file_node_get`].
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
                // Use `entry().or_insert()` so a caller who put a typed
                // wire key (e.g. "accountId", "create", "update",
                // "destroy", "ifInState") into `params.extra` cannot
                // silently clobber the typed args. Typed wins on collision.
                let args_obj = args
                    .as_object_mut()
                    .expect("file_node_set: args is constructed as Object");
                for (k, v) in map {
                    args_obj.entry(k).or_insert(v);
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
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:filenode`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `on_exists` is `Some` and serializing it fails (pathological
    ///   conditions only).
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::file_node_get`]. RFC 8620
    ///   §5.4 /copy adds method-level errors `fromAccountNotFound`,
    ///   `fromAccountNotSupportedByMethod`, and `anchorNotFound`; they
    ///   surface as
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError).
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
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:filenode`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::file_node_get`]. RFC 8620
    ///   §5.5 defines additional /query method-level errors
    ///   (`anchorNotFound`, `unsupportedFilter`, `unsupportedSort`,
    ///   `tooManyChanges`) that surface as
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError).
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
    ///
    /// `filter` and `sort` MUST match the `filter` / `sort` passed to the
    /// original `FileNode/query` call that returned `since_query_state` —
    /// RFC 8620 §5.6 is explicit that the server uses them to compute
    /// which entries entered or left the result set.
    ///
    /// `up_to_id` is the highest-index id the client has cached;
    /// `calculate_total` requests the new total result count.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `since_query_state` is the empty string (defence-in-depth
    ///   empty-state guard; see [`Self::file_node_changes`]).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:filenode`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::file_node_get`]. RFC 8620
    ///   §5.6 also defines `cannotCalculateChanges` (returned when the
    ///   server cannot honour the request given the supplied filter /
    ///   sort); it surfaces as
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError).
    pub async fn file_node_query_changes(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
        filter: Option<serde_json::Value>,
        sort: Option<serde_json::Value>,
        up_to_id: Option<&Id>,
        calculate_total: Option<bool>,
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
        if let Some(f) = filter {
            args["filter"] = f;
        }
        if let Some(s) = sort {
            args["sort"] = s;
        }
        if let Some(mc) = max_changes {
            args["maxChanges"] = mc.into();
        }
        if let Some(uti) = up_to_id {
            args["upToId"] = serde_json::to_value(uti).expect("Id Serialize is infallible");
        }
        if let Some(ct) = calculate_total {
            args["calculateTotal"] = ct.into();
        }
        let req = super::build_request("FileNode/queryChanges", args, super::USING_FILENODE);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Tests — see tests/integration_get_query.rs and tests/integration_set_copy.rs
// (wiremock-backed end-to-end)
// ---------------------------------------------------------------------------
//
// `file_node_get_request_shape`, `file_node_query_with_depth_includes_depth_in_args`,
// `file_node_query_without_depth_omits_depth`,
// `file_node_changes_request_includes_since_state`,
// `file_node_set_destroy_request_shape`,
// `file_node_copy_request_includes_from_account_id`,
// `file_node_copy_with_on_destroy_remove_children_includes_key`, and
// `file_node_copy_without_on_destroy_remove_children_omits_key` were vacuous:
// they hand-built `args` Values and fed them to `build_request`, never
// exercising the production `file_node_get`, `file_node_query`,
// `file_node_changes`, `file_node_set`, or `file_node_copy` builders.
// Deleted in JMAP-tco1.25.
//
// Real production-path coverage:
//   - tests/integration_get_query.rs (wiremock-backed FileNode/get and
//     FileNode/query)
//   - tests/integration_set_copy.rs (wiremock-backed FileNode/set and
//     FileNode/copy)
//
// Specific-flag passthrough coverage that may be lost is tracked
// under JMAP-uuoi for follow-up wiremock smoke tests.
//
// `build_request`, `CALL_ID`, and `USING_FILENODE` themselves have their
// own focused tests in `methods/mod.rs`.

#[cfg(test)]
mod tests {
    use super::super::{QueryResponse, SetResponse};
    use super::{FileNodeCopyParams, FileNodeOnExists, FileNodeSetParams};
    use jmap_types::Id;
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
            extra: serde_json::Map::new(),
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
            extra: serde_json::Map::new(),
        };
        let val = serde_json::to_value(&params).expect("serialize");
        assert_eq!(val["onDestroyRemoveChildren"], json!(false));
        assert_eq!(val["onExists"], json!("replace"));
        assert_eq!(val["compareCaseInsensitively"], json!(true));
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

    // ── Extras-preservation policy tests (JMAP-lbdy.9) ─────────────────
    //
    // For Serialize-only method-argument structs, the test constructs a
    // struct with a vendor field in `extra` and asserts that the field
    // flattens into the serialized JSON. Uses synthetic `acmeCorp*` keys
    // that are guaranteed not to appear in any draft-ietf-jmap-filenode
    // typed field — so the tests are independent of the crate under test.

    /// `FileNodeSetParams.extra` flattens into serialized JSON.
    #[test]
    fn file_node_set_params_propagates_vendor_extras() {
        let mut params = FileNodeSetParams::default();
        params
            .extra
            .insert("acmeCorpCascade".into(), json!("strict"));
        let v = serde_json::to_value(&params).expect("serialize FileNodeSetParams");
        assert_eq!(v["acmeCorpCascade"], json!("strict"));
    }

    /// `FileNodeCopyParams.extra` flattens into serialized JSON.
    #[test]
    fn file_node_copy_params_propagates_vendor_extras() {
        let mut extra = serde_json::Map::new();
        extra.insert("acmeCorpAudit".into(), json!(true));
        let params = FileNodeCopyParams {
            from_account_id: Id::from("acct-src"),
            extra,
        };
        let v = serde_json::to_value(&params).expect("serialize FileNodeCopyParams");
        assert_eq!(v["acmeCorpAudit"], json!(true));
    }
}
