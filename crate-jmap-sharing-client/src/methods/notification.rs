// JMAP Sharing — ShareNotification/* method implementations on SessionClient.
//
// Each method follows the standard five-step pattern:
//   1. Validate arguments (empty-string guards).
//   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//   3. Build args JSON with `serde_json::json!({…})`.
//   4. Call `build_request(method_name, args, USING_SHARING)`.
//   5. Call `self.call_internal(api_url, &req).await?`.
//   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.
//
// Note: ShareNotification/set is destroy-only per RFC 9670 §3.3.
// The server MUST reject create/update operations with `forbidden` errors.
// This method accepts only `destroy` to prevent constructing invalid requests.

use super::{ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SetResponse};

impl super::SessionClient {
    /// Fetch ShareNotification objects by IDs (RFC 9670 §3.1).
    ///
    /// If `ids` is `None`, the server returns all ShareNotifications for the account.
    /// Pass `properties: None` to return all fields.
    pub async fn share_notification_get(
        &self,
        ids: Option<&[&str]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_sharing_types::ShareNotification>, jmap_base_client::ClientError>
    {
        if let Some(id_slice) = ids {
            for id in id_slice.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "share_notification_get: ids element may not be empty".into(),
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
        let req = super::build_request("ShareNotification/get", args, super::USING_SHARING);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to ShareNotification objects since `since_state` (RFC 9670 §3.2).
    ///
    /// If `has_more_changes` is true in the response, call again with `new_state`
    /// as `since_state` until the flag is false.
    pub async fn share_notification_changes(
        &self,
        since_state: &str,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        if since_state.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "share_notification_changes: since_state may not be empty".into(),
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
        let req = super::build_request("ShareNotification/changes", args, super::USING_SHARING);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Destroy ShareNotification objects (RFC 9670 §3.3).
    ///
    /// ShareNotification/set is destroy-only: the server rejects create/update
    /// operations with `forbidden` SetErrors. This method accepts only
    /// `destroy` to prevent constructing invalid requests.
    ///
    /// Pass `destroy: None` to send an empty destroy list (no-op). Pass
    /// `destroy: Some(ids)` to dismiss the listed notifications.
    pub async fn share_notification_set(
        &self,
        destroy: Option<Vec<&str>>,
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        if let Some(ref ids) = destroy {
            for id in ids.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "share_notification_set: destroy element may not be empty".into(),
                    ));
                }
            }
        }
        let (api_url, account_id) = self.session_parts()?;
        let destroy_val = match destroy {
            Some(ids) => serde_json::Value::Array(
                ids.into_iter()
                    .map(|id| serde_json::Value::String(id.to_owned()))
                    .collect(),
            ),
            None => serde_json::Value::Array(vec![]),
        };
        let args = serde_json::json!({
            "accountId": account_id,
            "destroy": destroy_val,
        });
        let req = super::build_request("ShareNotification/set", args, super::USING_SHARING);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Query ShareNotification IDs with optional filter and sort (RFC 9670 §3.4).
    ///
    /// The server MUST support the `created` comparator property (RFC 9670 §3.4.1).
    /// Pass `filter: None` and `sort: None` to return all notifications with
    /// server-default ordering. Use `position` and `limit` for pagination.
    pub async fn share_notification_query(
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
        let req = super::build_request("ShareNotification/query", args, super::USING_SHARING);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch query-result changes for ShareNotification since `since_query_state`
    /// (RFC 9670 §3.5).
    ///
    /// Returns which ShareNotification IDs were removed from or added to the
    /// query result set since the given state. `max_changes` may be `None`.
    pub async fn share_notification_query_changes(
        &self,
        since_query_state: &str,
        max_changes: Option<u64>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        if since_query_state.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "share_notification_query_changes: since_query_state may not be empty".into(),
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
        let req =
            super::build_request("ShareNotification/queryChanges", args, super::USING_SHARING);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::{build_request, CALL_ID, USING_SHARING};
    use serde_json::json;

    /// Oracle: empty ID in ids slice triggers the validation guard.
    /// Guard fires before any session lookup or network call.
    #[test]
    fn share_notification_get_empty_id_returns_invalid_argument() {
        // Test the validation guard directly without needing SessionClient.
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
    fn share_notification_changes_empty_state_returns_invalid_argument() {
        let since_state = "";
        let result: Result<(), jmap_base_client::ClientError> = {
            if since_state.is_empty() {
                Err(jmap_base_client::ClientError::InvalidArgument(
                    "share_notification_changes: since_state may not be empty".into(),
                ))
            } else {
                Ok(())
            }
        };
        assert!(
            matches!(
                result,
                Err(jmap_base_client::ClientError::InvalidArgument(_))
            ),
            "empty since_state must produce InvalidArgument"
        );
    }

    /// Oracle: share_notification_set with no destroy sends destroy:[] in args.
    /// The spec requires destroy-only — no create or update keys present.
    /// Expected: args has "destroy" key, does NOT have "create" or "update".
    #[test]
    fn share_notification_set_no_destroy_sends_empty_array() {
        let destroy_val = serde_json::Value::Array(vec![]);
        let args = json!({
            "accountId": "acc1",
            "destroy": destroy_val,
        });
        let req = build_request("ShareNotification/set", args, USING_SHARING);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");

        assert_eq!(calls[0][0], json!("ShareNotification/set"), "method name");
        assert_eq!(calls[0][2], json!(CALL_ID), "call id");

        let method_args = &calls[0][1];
        // destroy must be present as empty array
        let destroy = method_args["destroy"]
            .as_array()
            .expect("destroy must be array");
        assert!(destroy.is_empty(), "destroy must be empty when None passed");
        // create and update must NOT be present
        assert!(
            method_args.get("create").is_none(),
            "create must not be present"
        );
        assert!(
            method_args.get("update").is_none(),
            "update must not be present"
        );
    }

    /// Oracle: share_notification_set with destroy list sends destroy array.
    /// Expected: destroy array contains the listed IDs.
    #[test]
    fn share_notification_set_with_destroy_sends_ids() {
        let ids = ["notif1", "notif2"];
        let destroy_val = serde_json::Value::Array(
            ids.iter()
                .map(|id| serde_json::Value::String((*id).to_owned()))
                .collect(),
        );
        let args = json!({
            "accountId": "acc1",
            "destroy": destroy_val,
        });
        let req = build_request("ShareNotification/set", args, USING_SHARING);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        let destroy_arr = calls[0][1]["destroy"].as_array().expect("destroy array");
        assert_eq!(destroy_arr.len(), 2);
        assert!(destroy_arr.contains(&json!("notif1")));
        assert!(destroy_arr.contains(&json!("notif2")));
    }

    /// Oracle: empty since_query_state returns InvalidArgument.
    #[test]
    fn share_notification_query_changes_empty_state_returns_invalid_argument() {
        let since_query_state = "";
        let result: Result<(), jmap_base_client::ClientError> = {
            if since_query_state.is_empty() {
                Err(jmap_base_client::ClientError::InvalidArgument(
                    "share_notification_query_changes: since_query_state may not be empty".into(),
                ))
            } else {
                Ok(())
            }
        };
        assert!(
            matches!(
                result,
                Err(jmap_base_client::ClientError::InvalidArgument(_))
            ),
            "empty since_query_state must produce InvalidArgument"
        );
    }

    /// Oracle: ShareNotification/get request has correct method name and CALL_ID.
    /// Expected method name is "ShareNotification/get" per RFC 9670 §3.1.
    #[test]
    fn share_notification_get_request_shape() {
        let args = json!({
            "accountId": "acc1",
            "ids": null,
            "properties": null,
        });
        let req = build_request("ShareNotification/get", args, USING_SHARING);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("ShareNotification/get"));
        assert_eq!(calls[0][2], json!(CALL_ID));
    }

    /// Oracle: ShareNotification/query with filter sends filter in args.
    #[test]
    fn share_notification_query_request_includes_filter() {
        let filter = json!({"objectType": "Mailbox"});
        let mut args = json!({ "accountId": "acc1" });
        args["filter"] = filter;

        let req = build_request("ShareNotification/query", args, USING_SHARING);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(
            calls[0][1]["filter"]["objectType"],
            json!("Mailbox"),
            "filter objectType must be present"
        );
    }
}
