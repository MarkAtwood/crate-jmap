//! JMAP Tasks — TaskNotification/* method implementations on SessionClient.
//!
//! TaskNotification/set is destroy-only. Any create or update entries from
//! the client would be rejected by the server with `forbidden`, but we
//! expose a typed API that only allows destroy to avoid client-side confusion.

use jmap_types::{Id, State};

use super::{ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SetResponse};

impl super::SessionClient {
    /// Fetch TaskNotification objects by IDs (draft-tasks-06 §5.2).
    ///
    /// If `ids` is `None`, the server returns all TaskNotifications for the account.
    pub async fn task_notification_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_tasks_types::TaskNotification>, jmap_base_client::ClientError>
    {
        let (api_url, account_id) = self.session_parts()?;
        // Omit `ids` / `properties` when None — see the matching comment on
        // `task_list_get` for the rationale (consistent with set/changes/query).
        let mut args = serde_json::json!({ "accountId": account_id });
        if let Some(id_slice) = ids {
            args["ids"] = serde_json::to_value(id_slice).expect("Id slice Serialize is infallible");
        }
        if let Some(props) = properties {
            args["properties"] = serde_json::Value::Array(
                props.iter().copied().map(serde_json::Value::from).collect(),
            );
        }
        let req = super::build_request("TaskNotification/get", args, super::USING_TASKS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to TaskNotification objects since `since_state`
    /// (draft-tasks-06 §5.3).
    pub async fn task_notification_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see task_list_changes.
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "task_notification_changes: since_state may not be empty".into(),
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
        let req = super::build_request("TaskNotification/changes", args, super::USING_TASKS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Destroy TaskNotification objects (draft-tasks-06 §5.4).
    ///
    /// **Destroy-only**: TaskNotification/set only supports `destroy`.
    /// The server creates notifications automatically; clients may only remove them.
    ///
    /// Passing an empty `destroy` list is valid and produces an empty /set response.
    pub async fn task_notification_set(
        &self,
        destroy: Vec<Id>,
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let args = serde_json::json!({
            "accountId": account_id,
            "destroy": destroy,
        });
        let req = super::build_request("TaskNotification/set", args, super::USING_TASKS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Query TaskNotification IDs with optional filter and sort
    /// (draft-tasks-06 §5.5).
    pub async fn task_notification_query(
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
        let req = super::build_request("TaskNotification/query", args, super::USING_TASKS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch query-result changes for TaskNotification since `since_query_state`
    /// (draft-tasks-06 §5.6).
    pub async fn task_notification_query_changes(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see task_list_changes.
        if since_query_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "task_notification_query_changes: since_query_state may not be empty".into(),
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
        let req = super::build_request("TaskNotification/queryChanges", args, super::USING_TASKS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

// ---------------------------------------------------------------------------
// Tests — see tests/task_notification_tests.rs (wiremock-backed end-to-end)
// ---------------------------------------------------------------------------
//
// `task_notification_set_destroy_only_serialization` and
// `task_notification_set_uses_tasks_capability` were vacuous: they
// hand-built `args` Values and fed them to `build_request`, never
// exercising the production `task_notification_set` builder. Deleted in
// JMAP-tco1.20.
//
// Real production-path coverage:
//   - task_notification_get_round_trip
//   - task_notification_changes_round_trip
//   - task_notification_set_destroy_only_wire_format
//   - task_notification_set_empty_destroy_succeeds
//   - task_notification_query_with_filter
//   - task_notification_query_changes_round_trip
// in tests/task_notification_tests.rs.
//
// Specific-flag passthrough coverage that may be lost is tracked
// under JMAP-uuoi for follow-up wiremock smoke tests.
//
// `build_request`, `CALL_ID`, and `USING_TASKS` themselves have their
// own focused tests in `methods/mod.rs`.
//
// The InvalidArgument guard for empty since_state lives in
// task_notification_changes production code; testing it requires a
// wiremock-backed async harness. See JMAP-sc1b.64.
