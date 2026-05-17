//! JMAP Tasks — Task/* method implementations on SessionClient.

use std::collections::HashMap;

use jmap_types::{Id, PatchObject, State};

use super::{ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SetResponse};

impl super::SessionClient {
    /// Fetch Task objects by IDs (draft-tasks-06 §4.5).
    ///
    /// If `ids` is `None`, the server returns all Tasks for the account,
    /// SUBJECT TO the server's `maxObjectsInGet` cap (RFC 8620 §5.1).
    /// For production use, scope the result set via the corresponding
    /// /query method first and pass explicit ids here to avoid
    /// `requestTooLarge` errors when the account holds more objects
    /// than the cap.
    /// Pass `properties: None` to return all fields.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:tasks`.
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
    pub async fn task_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_tasks_types::Task>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        // Omit `ids` / `properties` when None — see the matching comment on
        // `task_list_get` for the rationale (consistent with set/changes/query).
        let mut args = serde_json::json!({ "accountId": account_id });
        if let Some(id_slice) = ids {
            args["ids"] = serde_json::to_value(id_slice).expect("Id slice Serialize is infallible");
        }
        if let Some(props) = properties {
            args["properties"] =
                serde_json::to_value(props).expect("&[&str] Serialize is infallible");
        }
        let req = super::build_request("Task/get", args, super::USING_TASKS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to Task objects since `since_state` (draft-tasks-06 §4.6).
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
    ///   `urn:ietf:params:jmap:tasks`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::task_get`].
    pub async fn task_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see task_list_changes.
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "task_changes: since_state may not be empty".into(),
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
        let req = super::build_request("Task/changes", args, super::USING_TASKS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create, update, or destroy Task objects (draft-tasks-06 §4.7).
    ///
    /// `update` is `Option<HashMap<Id, PatchObject>>` (RFC 8620 §5.3). Wire
    /// format is unchanged from a plain JSON object because [`PatchObject`]
    /// is `#[serde(transparent)]`; the typed parameter binds the JSON Pointer
    /// key + null-leaf removal contract to the type system.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:tasks`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `update` is `Some` and `serde_json::to_value` fails on the
    ///   patch map (pathological conditions only — allocation failure,
    ///   or a `PatchObject` whose JSON tree exceeds `serde_json`'s
    ///   recursion limit). The transient memory peak for very large
    ///   `update` maps is roughly 3-4× the `HashMap`'s in-memory size
    ///   (source map + `serde_json::Value` tree + serialized `Vec<u8>`
    ///   body); callers dealing with thousands of patches per call may
    ///   prefer to batch.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::task_get`].
    pub async fn task_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<HashMap<Id, PatchObject>>,
        destroy: Option<Vec<Id>>,
    ) -> Result<SetResponse<jmap_tasks_types::Task>, jmap_base_client::ClientError> {
        if create.is_none() && update.is_none() && destroy.is_none() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "task_set: at least one of create, update, destroy must be Some \
                 (an all-None /set is a no-op round-trip)"
                    .into(),
            ));
        }
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
                    "task_set: serializing update map failed: {e}"
                ))
            })?;
        }
        if let Some(d) = destroy {
            args["destroy"] = serde_json::to_value(&d).expect("Id Vec Serialize is infallible");
        }
        let req = super::build_request("Task/set", args, super::USING_TASKS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Copy Tasks from another account (draft-tasks-06 §4.8).
    ///
    /// `from_account_id` is the source account. The tasks are copied into the
    /// current primary Tasks account.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:tasks`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::task_get`]. RFC 8620 §5.4
    ///   /copy adds method-level errors `fromAccountNotFound`,
    ///   `fromAccountNotSupportedByMethod`, and `anchorNotFound`; they
    ///   surface as
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError).
    pub async fn task_copy(
        &self,
        from_account_id: &Id,
        create: serde_json::Value,
    ) -> Result<SetResponse<jmap_tasks_types::Task>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let args = serde_json::json!({
            "fromAccountId": from_account_id,
            "accountId": account_id,
            "create": create,
        });
        let req = super::build_request("Task/copy", args, super::USING_TASKS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Query Task IDs with optional filter and sort (draft-tasks-06 §4.13).
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:tasks`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::task_get`]. RFC 8620 §5.5
    ///   defines additional /query method-level errors
    ///   (`anchorNotFound`, `unsupportedFilter`, `unsupportedSort`,
    ///   `tooManyChanges`) that surface as
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError).
    pub async fn task_query(
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
        let req = super::build_request("Task/query", args, super::USING_TASKS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch query-result changes for Task since `since_query_state`
    /// (draft-tasks-06 §4.14).
    ///
    /// `filter` and `sort` MUST match the `filter` / `sort` passed to the
    /// original `Task/query` call that returned `since_query_state` —
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
    ///   empty-state guard; see [`Self::task_changes`]).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:tasks`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::task_get`]. RFC 8620 §5.6
    ///   also defines `cannotCalculateChanges` (returned when the
    ///   server cannot honour the request given the supplied filter /
    ///   sort); it surfaces as
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError).
    pub async fn task_query_changes(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
        filter: Option<serde_json::Value>,
        sort: Option<serde_json::Value>,
        up_to_id: Option<&Id>,
        calculate_total: Option<bool>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see task_list_changes.
        if since_query_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "task_query_changes: since_query_state may not be empty".into(),
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
        let req = super::build_request("Task/queryChanges", args, super::USING_TASKS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

// ---------------------------------------------------------------------------
// Tests — see tests/task_tests.rs (wiremock-backed end-to-end)
// ---------------------------------------------------------------------------
//
// `task_copy_request_includes_from_account_id` was vacuous: it hand-built
// `args` Values and fed them to `build_request`, never exercising the
// production `task_copy` builder. Deleted in JMAP-tco1.20.
//
// Real production-path coverage:
//   - task_get_sends_ids_and_properties
//   - task_get_all_ids_null
//   - task_changes_paginated
//   - task_set_create_round_trip
//   - task_copy_includes_from_account_id
//   - task_query_with_filter
//   - task_query_changes_round_trip
// in tests/task_tests.rs.
//
// Specific-flag passthrough coverage that may be lost is tracked
// under JMAP-uuoi for follow-up wiremock smoke tests.
//
// `build_request`, `CALL_ID`, and `USING_TASKS` themselves have their
// own focused tests in `methods/mod.rs`.
//
// The InvalidArgument guards for empty since_state, from_account_id, and
// since_query_state live in task_changes / task_copy / task_query_changes
// production code; testing them requires a wiremock-backed async harness.
// See JMAP-sc1b.64.
//
// The `task_get_empty_id_returns_invalid_argument` inline smoke test was
// removed by the JMAP-6by7.5 typed-Id refactor. It was vacuous because
// it only iterated a local `&[""]` slice and asserted `is_empty()` found
// the empty value, without invoking any production method. Under typed
// `&[Id]` parameters, an empty-Id input is impossible to express through
// the API (`Id::new_validated("")` returns `Err` at the call site) so the
// bug it pretended to test is unrepresentable.
