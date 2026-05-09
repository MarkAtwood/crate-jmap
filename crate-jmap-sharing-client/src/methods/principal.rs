// JMAP Sharing — Principal/* method implementations on SessionClient.
//
// Each method follows the standard five-step pattern:
//   1. Validate arguments (defence-in-depth empty-state guards).
//   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//   3. Build args JSON with `serde_json::json!({…})`.
//   4. Call `build_request(method_name, args, USING_SHARING)`.
//   5. Call `self.call_internal(api_url, &req).await?`.
//   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.

use std::collections::HashMap;

use jmap_types::{Id, PatchObject, State};

use super::{ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SetResponse};

impl super::SessionClient {
    /// Fetch Principal objects by IDs (RFC 9670 §2.1).
    ///
    /// If `ids` is `None`, the server returns all Principals for the account.
    /// Pass `properties: None` to return all fields.
    pub async fn principal_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_sharing_types::Principal>, jmap_base_client::ClientError> {
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
        let req = super::build_request("Principal/get", args, super::USING_SHARING);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to Principal objects since `since_state` (RFC 9670 §2.2).
    ///
    /// If `has_more_changes` is true in the response, call again with `new_state`
    /// as `since_state` until the flag is false.
    ///
    /// Note: servers backed by an external directory may return
    /// `cannotCalculateChanges` if change tracking is unavailable.
    pub async fn principal_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: even with the typed-`State` parameter (a transparent
        // newtype around `String`), an empty state token is still a logically
        // invalid value that should be caught client-side rather than producing
        // a confusing server-side `cannotCalculateChanges` error.
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "principal_changes: since_state may not be empty".into(),
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
        let req = super::build_request("Principal/changes", args, super::USING_SHARING);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create, update, or destroy Principal objects (RFC 9670 §2.3).
    ///
    /// Servers may reject create/update operations with `forbidden`. Only
    /// `name`, `description`, and `timeZone` on the caller's own Principal
    /// are guaranteed settable (if the server supports it at all).
    ///
    /// Pass `create`, `update`, and/or `destroy` as needed. All three are
    /// optional; pass `None` to omit any operation from the request.
    ///
    /// `update` is `Option<HashMap<Id, PatchObject>>` (RFC 8620 §5.3). Wire
    /// format is unchanged from a plain JSON object because [`PatchObject`]
    /// is `#[serde(transparent)]`; the typed parameter binds the JSON Pointer
    /// key + null-leaf removal contract to the type system.
    pub async fn principal_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<HashMap<Id, PatchObject>>,
        destroy: Option<Vec<Id>>,
    ) -> Result<SetResponse<jmap_sharing_types::Principal>, jmap_base_client::ClientError> {
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
                    "principal_set: serializing update map failed: {e}"
                ))
            })?;
        }
        if let Some(d) = destroy {
            args["destroy"] = serde_json::to_value(&d).expect("Id Vec Serialize is infallible");
        }
        let req = super::build_request("Principal/set", args, super::USING_SHARING);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Query Principal IDs with optional filter and sort (RFC 9670 §2.4).
    ///
    /// Pass `filter: None` and `sort: None` to return all Principals with
    /// server-default ordering. Use `position` and `limit` for pagination.
    pub async fn principal_query(
        &self,
        filter: Option<serde_json::Value>,
        sort: Option<serde_json::Value>,
        position: Option<u64>,
        limit: Option<u64>,
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
        let req = super::build_request("Principal/query", args, super::USING_SHARING);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch query-result changes for Principal since `since_query_state`
    /// (RFC 9670 §2.5).
    ///
    /// Returns which Principal IDs were removed from or added to the query
    /// result set since the given state. `max_changes` may be `None`.
    pub async fn principal_query_changes(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `principal_changes`.
        if since_query_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "principal_query_changes: since_query_state may not be empty".into(),
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
        let req = super::build_request("Principal/queryChanges", args, super::USING_SHARING);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

// ---------------------------------------------------------------------------
// Tests — see tests/principal_*_tests.rs (wiremock-backed end-to-end)
// ---------------------------------------------------------------------------
//
// `principal_get_request_shape`,
// `principal_changes_request_includes_since_state`,
// `principal_set_destroy_request_shape`, and
// `principal_query_request_includes_filter` were vacuous: they
// hand-built `args` Values and fed them to `build_request`, never
// exercising the production `principal_get` / `principal_changes` /
// `principal_set` / `principal_query` / `principal_query_changes`
// builders. Deleted in JMAP-tco1.30.
//
// Real production-path coverage:
//   - `principal_get_round_trip`,
//     `principal_get_specific_ids_sends_array`, and
//     `principal_changes_sends_since_state` in
//     tests/principal_get_changes_tests.rs
//   - `principal_set_destroy_round_trip` and
//     `principal_set_create_returns_forbidden` in
//     tests/principal_set_tests.rs
//   - `principal_query_with_filter` and
//     `principal_query_changes_round_trip` in
//     tests/principal_query_tests.rs
//
// Specific-flag passthrough coverage that may be lost is tracked
// under JMAP-uuoi for follow-up wiremock smoke tests.
//
// `build_request`, `CALL_ID`, and `USING_SHARING` themselves have their
// own focused tests in `methods/mod.rs`.
//
// Inline guard smoke tests (e.g. `principal_get_empty_id_returns_invalid_argument`,
// `principal_changes_empty_since_state_returns_invalid_argument`,
// `principal_query_changes_empty_state_returns_invalid_argument`) were
// removed earlier by the JMAP-6by7.7 typed-Id refactor. They were
// vacuous because they only iterated a local `&[""]` slice (or
// duplicated the guard's `is_empty()` check) and asserted `is_empty()`
// found the empty value, without invoking any production method. Under
// typed `&[Id]` / `Vec<Id>` parameters, an empty-Id input is impossible
// to express through the API (`Id::new_validated("")` returns `Err` at
// the call site) so the bug they pretended to test is unrepresentable.
// Defence-in-depth empty-state guards still live in the production code
// (`principal_changes`, `principal_query_changes`) using
// `as_ref().is_empty()`.
