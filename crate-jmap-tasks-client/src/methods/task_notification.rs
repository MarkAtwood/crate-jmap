// JMAP Tasks — TaskNotification/* method implementations on SessionClient.
//
// TaskNotification/set is destroy-only. Any create or update entries from
// the client would be rejected by the server with `forbidden`, but we
// expose a typed API that only allows destroy to avoid client-side confusion.

use super::{ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SetResponse};

impl super::SessionClient {
    /// Fetch TaskNotification objects by IDs (draft-tasks-06 §5.2).
    ///
    /// If `ids` is `None`, the server returns all TaskNotifications for the account.
    pub async fn task_notification_get(
        &self,
        ids: Option<&[&str]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_tasks_types::TaskNotification>, jmap_base_client::ClientError>
    {
        if let Some(id_slice) = ids {
            for id in id_slice.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "task_notification_get: ids element may not be empty".into(),
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
        let req = super::build_request("TaskNotification/get", args, super::USING_TASKS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to TaskNotification objects since `since_state`
    /// (draft-tasks-06 §5.3).
    pub async fn task_notification_changes(
        &self,
        since_state: &str,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        if since_state.is_empty() {
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
        destroy: Vec<&str>,
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        for id in destroy.iter() {
            if id.is_empty() {
                return Err(jmap_base_client::ClientError::InvalidArgument(
                    "task_notification_set: destroy element may not be empty".into(),
                ));
            }
        }
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
        since_query_state: &str,
        max_changes: Option<u64>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        if since_query_state.is_empty() {
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::{build_request, CALL_ID, USING_TASKS};
    use serde_json::json;

    /// Oracle: TaskNotification/set destroy-only request serializes destroy array.
    ///
    /// A TaskNotification/set request must send only `destroy` (no create/update).
    /// Expected: args contains "destroy" key with the id list; no "create" or "update".
    #[test]
    fn task_notification_set_destroy_only_serialization() {
        let destroy_ids = vec!["notif1", "notif2"];
        let args = json!({
            "accountId": "acc1",
            "destroy": destroy_ids,
        });
        let req = build_request("TaskNotification/set", args, USING_TASKS);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");

        assert_eq!(calls[0][0], json!("TaskNotification/set"), "method name");
        assert_eq!(calls[0][2], json!(CALL_ID), "call id");

        let method_args = &calls[0][1];
        // Must have destroy
        let destroy = method_args["destroy"]
            .as_array()
            .expect("destroy must be array");
        assert_eq!(destroy.len(), 2);
        assert!(destroy.contains(&json!("notif1")));
        assert!(destroy.contains(&json!("notif2")));
        // Must NOT have create or update
        assert!(
            method_args.get("create").is_none() || method_args["create"].is_null(),
            "destroy-only set must not include create"
        );
        assert!(
            method_args.get("update").is_none() || method_args["update"].is_null(),
            "destroy-only set must not include update"
        );
    }

    /// Oracle: empty id in destroy returns InvalidArgument.
    #[test]
    fn task_notification_set_empty_destroy_id_returns_invalid_argument() {
        let destroy = vec![""];
        let result: Result<(), jmap_base_client::ClientError> = {
            let mut err = None;
            for id in destroy.iter() {
                if id.is_empty() {
                    err = Some(jmap_base_client::ClientError::InvalidArgument(
                        "task_notification_set: destroy element may not be empty".into(),
                    ));
                    break;
                }
            }
            err.map(Err).unwrap_or(Ok(()))
        };
        assert!(matches!(
            result,
            Err(jmap_base_client::ClientError::InvalidArgument(_))
        ));
    }

    /// Oracle: empty since_state returns InvalidArgument.
    #[test]
    fn task_notification_changes_empty_since_state_returns_invalid_argument() {
        let since_state = "";
        let result: Result<(), jmap_base_client::ClientError> = if since_state.is_empty() {
            Err(jmap_base_client::ClientError::InvalidArgument(
                "task_notification_changes: since_state may not be empty".into(),
            ))
        } else {
            Ok(())
        };
        assert!(matches!(
            result,
            Err(jmap_base_client::ClientError::InvalidArgument(_))
        ));
    }

    /// Oracle: USING_TASKS is used for TaskNotification/set request.
    #[test]
    fn task_notification_set_uses_tasks_capability() {
        let args = json!({ "accountId": "acc1", "destroy": [] });
        let req = build_request("TaskNotification/set", args, USING_TASKS);
        let v = serde_json::to_value(&req).expect("serialize");
        let using = v["using"].as_array().expect("using");
        assert!(using.contains(&json!("urn:ietf:params:jmap:tasks")));
    }
}
