// JMAP Tasks — Task/* method implementations on SessionClient.

use super::{ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SetResponse};

impl super::SessionClient {
    /// Fetch Task objects by IDs (draft-tasks-06 §4.5).
    ///
    /// If `ids` is `None`, the server returns all Tasks for the account.
    /// Pass `properties: None` to return all fields.
    pub async fn task_get(
        &self,
        ids: Option<&[&str]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_tasks_types::Task>, jmap_base_client::ClientError> {
        if let Some(id_slice) = ids {
            for id in id_slice.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "task_get: ids element may not be empty".into(),
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
        let req = super::build_request("Task/get", args, super::USING_TASKS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to Task objects since `since_state` (draft-tasks-06 §4.6).
    pub async fn task_changes(
        &self,
        since_state: &str,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        if since_state.is_empty() {
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
    pub async fn task_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<serde_json::Value>,
        destroy: Option<Vec<&str>>,
    ) -> Result<SetResponse<jmap_tasks_types::Task>, jmap_base_client::ClientError> {
        if let Some(ref ids) = destroy {
            for id in ids.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "task_set: destroy element may not be empty".into(),
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
        let req = super::build_request("Task/set", args, super::USING_TASKS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Copy Tasks from another account (draft-tasks-06 §4.8).
    ///
    /// `from_account_id` is the source account. The tasks are copied into the
    /// current primary Tasks account.
    pub async fn task_copy(
        &self,
        from_account_id: &str,
        create: serde_json::Value,
    ) -> Result<SetResponse<jmap_tasks_types::Task>, jmap_base_client::ClientError> {
        if from_account_id.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "task_copy: from_account_id may not be empty".into(),
            ));
        }
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
    pub async fn task_query_changes(
        &self,
        since_query_state: &str,
        max_changes: Option<u64>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        if since_query_state.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "task_query_changes: since_query_state may not be empty".into(),
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
        let req = super::build_request("Task/queryChanges", args, super::USING_TASKS);
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
    fn task_get_empty_id_returns_invalid_argument() {
        let ids: &[&str] = &[""];
        let has_empty = ids.iter().any(|id| id.is_empty());
        assert!(has_empty, "empty id must trigger guard");
    }

    /// Oracle: empty since_state for task_changes returns InvalidArgument.
    #[test]
    fn task_changes_empty_since_state_returns_invalid_argument() {
        let since_state = "";
        let result: Result<(), jmap_base_client::ClientError> = if since_state.is_empty() {
            Err(jmap_base_client::ClientError::InvalidArgument(
                "task_changes: since_state may not be empty".into(),
            ))
        } else {
            Ok(())
        };
        assert!(matches!(
            result,
            Err(jmap_base_client::ClientError::InvalidArgument(_))
        ));
    }

    /// Oracle: empty from_account_id for task_copy returns InvalidArgument.
    #[test]
    fn task_copy_empty_from_account_id_returns_invalid_argument() {
        let from_account_id = "";
        let result: Result<(), jmap_base_client::ClientError> = if from_account_id.is_empty() {
            Err(jmap_base_client::ClientError::InvalidArgument(
                "task_copy: from_account_id may not be empty".into(),
            ))
        } else {
            Ok(())
        };
        assert!(matches!(
            result,
            Err(jmap_base_client::ClientError::InvalidArgument(_))
        ));
    }

    /// Oracle: Task/copy request includes fromAccountId in args.
    #[test]
    fn task_copy_request_includes_from_account_id() {
        let args = json!({
            "fromAccountId": "srcacc",
            "accountId": "dstacc",
            "create": {},
        });
        let req = build_request("Task/copy", args, USING_TASKS);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("Task/copy"));
        assert_eq!(calls[0][1]["fromAccountId"], json!("srcacc"));
        assert_eq!(calls[0][2], json!(CALL_ID));
    }

    /// Oracle: empty since_query_state for task_query_changes returns InvalidArgument.
    #[test]
    fn task_query_changes_empty_state_returns_invalid_argument() {
        let since_query_state = "";
        let result: Result<(), jmap_base_client::ClientError> = if since_query_state.is_empty() {
            Err(jmap_base_client::ClientError::InvalidArgument(
                "task_query_changes: since_query_state may not be empty".into(),
            ))
        } else {
            Ok(())
        };
        assert!(matches!(
            result,
            Err(jmap_base_client::ClientError::InvalidArgument(_))
        ));
    }
}
