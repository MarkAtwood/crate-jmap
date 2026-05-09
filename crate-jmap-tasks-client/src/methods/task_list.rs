// JMAP Tasks — TaskList/* method implementations on SessionClient.
//
// Each method follows the standard five-step pattern:
//   1. Validate arguments (defence-in-depth empty-state guards).
//   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//   3. Build args JSON with `serde_json::json!({…})`.
//   4. Call `build_request(method_name, args, USING_TASKS)`.
//   5. Call `self.call_internal(api_url, &req).await?`.
//   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.

use std::collections::HashMap;

use jmap_types::{Id, PatchObject, State};

use super::{ChangesResponse, GetResponse, SetResponse};

impl super::SessionClient {
    /// Fetch TaskList objects by IDs (draft-tasks-06 §3.5).
    ///
    /// If `ids` is `None`, the server returns all TaskLists for the account.
    /// Pass `properties: None` to return all fields.
    pub async fn task_list_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_tasks_types::TaskList>, jmap_base_client::ClientError> {
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
        let req = super::build_request("TaskList/get", args, super::USING_TASKS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to TaskList objects since `since_state` (draft-tasks-06 §3.6).
    ///
    /// If `has_more_changes` is true in the response, call again with `new_state`
    /// as `since_state` until the flag is false.
    pub async fn task_list_changes(
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
                "task_list_changes: since_state may not be empty".into(),
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
        let req = super::build_request("TaskList/changes", args, super::USING_TASKS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create, update, or destroy TaskList objects (draft-tasks-06 §3.7).
    ///
    /// `on_destroy_remove_tasks`: if `true`, any tasks in destroyed task lists
    /// are also destroyed. If `false` (default), attempting to destroy a task list
    /// that still contains tasks returns a `taskListHasTasks` error for that id.
    ///
    /// Pass `create`, `update`, and/or `destroy` as needed. All three are
    /// optional; pass `None` to omit any operation from the request.
    ///
    /// `update` is `Option<HashMap<Id, PatchObject>>` (RFC 8620 §5.3). Wire
    /// format is unchanged from a plain JSON object because [`PatchObject`]
    /// is `#[serde(transparent)]`; the typed parameter binds the JSON Pointer
    /// key + null-leaf removal contract to the type system.
    pub async fn task_list_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<HashMap<Id, PatchObject>>,
        destroy: Option<Vec<Id>>,
        on_destroy_remove_tasks: Option<bool>,
    ) -> Result<SetResponse<jmap_tasks_types::TaskList>, jmap_base_client::ClientError> {
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
                    "task_list_set: serializing update map failed: {e}"
                ))
            })?;
        }
        if let Some(d) = destroy {
            args["destroy"] = serde_json::to_value(&d).expect("Id Vec Serialize is infallible");
        }
        if let Some(v) = on_destroy_remove_tasks {
            args["onDestroyRemoveTasks"] = v.into();
        }
        let req = super::build_request("TaskList/set", args, super::USING_TASKS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::{build_request, CALL_ID, USING_TASKS};
    use serde_json::json;

    // The `task_list_get_empty_id_returns_invalid_argument` inline smoke test
    // was removed by the JMAP-6by7.5 typed-Id refactor. It was vacuous because
    // it only iterated a local `&[""]` slice and asserted `is_empty()` found
    // the empty value, without invoking any production method. Under typed
    // `&[Id]` parameters, an empty-Id input is impossible to express through
    // the API (`Id::new_validated("")` returns `Err` at the call site) so the
    // bug it pretended to test is unrepresentable.

    // The InvalidArgument guard for empty since_state lives in task_list_changes
    // production code; testing it requires a wiremock-backed async harness.
    // See JMAP-sc1b.64.

    /// Oracle: TaskList/set with onDestroyRemoveTasks sends the field in args.
    #[test]
    fn task_list_set_on_destroy_remove_tasks_in_args() {
        let mut args = json!({ "accountId": "acc1" });
        args["destroy"] = json!(["list1"]);
        args["onDestroyRemoveTasks"] = json!(true);
        let req = build_request("TaskList/set", args, USING_TASKS);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][1]["onDestroyRemoveTasks"], json!(true));
        assert_eq!(calls[0][2], json!(CALL_ID));
    }
}
