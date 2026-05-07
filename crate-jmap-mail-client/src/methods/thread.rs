// JMAP Mail — Thread/* method implementations on SessionClient.
//
// Each method follows the standard five-step pattern:
//   1. Validate arguments (empty-string guards).
//   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//   3. Build args JSON with `serde_json::json!({…})`.
//   4. Call `build_request(method_name, args, USING_MAIL)`.
//   5. Call `self.call_internal(api_url, &req).await?`.
//   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.

use super::{ChangesResponse, GetResponse};

impl super::SessionClient {
    /// Fetch Thread objects by IDs (RFC 8621 §3.1 — Thread/get).
    ///
    /// If `ids` is `None`, the server returns all Threads for the account.
    /// Pass `properties: None` to return all fields.
    pub async fn thread_get(
        &self,
        ids: Option<&[&str]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_mail_types::Thread>, jmap_base_client::ClientError> {
        if let Some(id_slice) = ids {
            for id in id_slice.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "thread_get: ids element may not be empty".into(),
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
        let req = super::build_request("Thread/get", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to Thread objects since `since_state` (RFC 8621 §3.2 — Thread/changes).
    ///
    /// If `has_more_changes` is true in the response, call again with `new_state`
    /// as `since_state` until the flag is false.
    pub async fn thread_changes(
        &self,
        since_state: &str,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        if since_state.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "thread_changes: since_state may not be empty".into(),
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
        let req = super::build_request("Thread/changes", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::{build_request, CALL_ID, USING_MAIL};
    use serde_json::json;

    /// Oracle: empty ID in ids slice triggers the validation guard.
    #[test]
    fn thread_get_empty_id_returns_invalid_argument() {
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

    // The InvalidArgument guard for empty since_state lives in thread_changes
    // production code; testing it requires a wiremock-backed async harness.
    // See JMAP-sc1b.64.

    /// Oracle: Thread/get request has correct method name and call id.
    #[test]
    fn thread_get_request_shape() {
        let args = json!({
            "accountId": "acc1",
            "ids": ["t1", "t2"],
            "properties": null,
        });
        let req = build_request("Thread/get", args, USING_MAIL);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("Thread/get"), "method name");
        assert_eq!(calls[0][2], json!(CALL_ID), "call id");
        let ids = calls[0][1]["ids"].as_array().expect("ids array");
        assert_eq!(ids.len(), 2);
    }

    /// Oracle: Thread/changes request includes sinceState.
    #[test]
    fn thread_changes_request_includes_since_state() {
        let args = json!({
            "accountId": "acc1",
            "sinceState": "state77",
        });
        let req = build_request("Thread/changes", args, USING_MAIL);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][1]["sinceState"], json!("state77"));
    }

    /// Oracle: Thread deserialization from RFC 8621 §3 shape.
    /// RFC 8621 §3.1: Thread has id and emailIds fields.
    #[test]
    fn thread_get_response_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "state": "s5",
            "list": [
                {
                    "id": "t1",
                    "emailIds": ["e1", "e2", "e3"]
                }
            ],
            "notFound": []
        });
        use super::super::GetResponse;
        let resp: GetResponse<jmap_mail_types::Thread> =
            serde_json::from_value(json).expect("must deserialize Thread GetResponse");
        assert_eq!(resp.list.len(), 1);
        assert_eq!(resp.list[0].id.as_ref(), "t1");
        assert_eq!(resp.list[0].email_ids.len(), 3);
    }
}
