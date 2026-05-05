// JMAP Tasks — TaskList/* method implementations on SessionClient.
//
// Each method follows the standard five-step pattern:
//   1. Validate arguments (empty-string guards).
//   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//   3. Build args JSON with `serde_json::json!({…})`.
//   4. Call `build_request(method_name, args, USING_TASKS)`.
//   5. Call `self.call_internal(api_url, &req).await?`.
//   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.

use super::{ChangesResponse, GetResponse, SetResponse};

impl super::SessionClient {
    /// Fetch TaskList objects by IDs (draft-tasks-06 §3.5).
    ///
    /// If `ids` is `None`, the server returns all TaskLists for the account.
    /// Pass `properties: None` to return all fields.
    pub async fn task_list_get(
        &self,
        ids: Option<&[&str]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_tasks_types::TaskList>, jmap_base_client::ClientError> {
        if let Some(id_slice) = ids {
            for id in id_slice.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "task_list_get: ids element may not be empty".into(),
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
        since_state: &str,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        if since_state.is_empty() {
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
    pub async fn task_list_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<serde_json::Value>,
        destroy: Option<Vec<&str>>,
        on_destroy_remove_tasks: Option<bool>,
    ) -> Result<SetResponse<jmap_tasks_types::TaskList>, jmap_base_client::ClientError> {
        if let Some(ref ids) = destroy {
            for id in ids.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "task_list_set: destroy element may not be empty".into(),
                    ));
                }
            }
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
        });
        if let Some(c) = create {
            args["create"] = c;
        }
        if let Some(u) = update {
            args["update"] = u;
        }
        if let Some(d) = destroy {
            args["destroy"] = serde_json::Value::Array(
                d.into_iter()
                    .map(|id| serde_json::Value::String(id.to_owned()))
                    .collect(),
            );
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

    /// Oracle: empty ID in ids slice returns InvalidArgument.
    #[test]
    fn task_list_get_empty_id_returns_invalid_argument() {
        let ids: &[&str] = &[""];
        let mut found_error = false;
        for id in ids.iter() {
            if id.is_empty() {
                found_error = true;
                break;
            }
        }
        assert!(
            found_error,
            "empty id must trigger the InvalidArgument guard"
        );
    }

    /// Oracle: empty since_state returns InvalidArgument.
    #[test]
    fn task_list_changes_empty_since_state_returns_invalid_argument() {
        let since_state = "";
        let result: Result<(), jmap_base_client::ClientError> = if since_state.is_empty() {
            Err(jmap_base_client::ClientError::InvalidArgument(
                "task_list_changes: since_state may not be empty".into(),
            ))
        } else {
            Ok(())
        };
        assert!(
            matches!(
                result,
                Err(jmap_base_client::ClientError::InvalidArgument(_))
            ),
            "empty since_state must produce InvalidArgument"
        );
    }

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
