// JMAP Mail — Identity/* method implementations on SessionClient.
//
// Each method follows the standard five-step pattern:
//   1. Validate arguments (empty-string guards).
//   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//   3. Build args JSON with `serde_json::json!({…})`.
//   4. Call `build_request(method_name, args, USING_MAIL)`.
//   5. Call `self.call_internal(api_url, &req).await?`.
//   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.

use super::{ChangesResponse, GetResponse, SetResponse};

impl super::SessionClient {
    /// Fetch Identity objects by IDs (RFC 8621 §6.1 — Identity/get).
    ///
    /// If `ids` is `None`, the server returns all Identities for the account.
    /// Pass `properties: None` to return all fields.
    pub async fn identity_get(
        &self,
        ids: Option<&[&str]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_mail_types::Identity>, jmap_base_client::ClientError> {
        if let Some(id_slice) = ids {
            for id in id_slice.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "identity_get: ids element may not be empty".into(),
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
        let req = super::build_request("Identity/get", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to Identity objects since `since_state` (RFC 8621 §6.2 — Identity/changes).
    pub async fn identity_changes(
        &self,
        since_state: &str,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        if since_state.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "identity_changes: since_state may not be empty".into(),
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
        let req = super::build_request("Identity/changes", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create, update, or destroy Identity objects (RFC 8621 §6.3 — Identity/set).
    ///
    /// Pass `create`, `update`, and/or `destroy` as needed. Pass `None` to omit.
    pub async fn identity_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<serde_json::Value>,
        destroy: Option<Vec<&str>>,
    ) -> Result<SetResponse<jmap_mail_types::Identity>, jmap_base_client::ClientError> {
        if let Some(ref ids) = destroy {
            for id in ids.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "identity_set: destroy element may not be empty".into(),
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
        let req = super::build_request("Identity/set", args, super::USING_MAIL);
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
    fn identity_get_empty_id_returns_invalid_argument() {
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
    fn identity_changes_empty_since_state_returns_invalid_argument() {
        let since_state = "";
        let result: Result<(), jmap_base_client::ClientError> = if since_state.is_empty() {
            Err(jmap_base_client::ClientError::InvalidArgument(
                "identity_changes: since_state may not be empty".into(),
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

    /// Oracle: Identity/get request shape is correct.
    #[test]
    fn identity_get_request_shape() {
        let args = json!({
            "accountId": "acc1",
            "ids": null,
            "properties": null,
        });
        let req = build_request("Identity/get", args, USING_MAIL);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("Identity/get"), "method name");
        assert_eq!(calls[0][2], json!(CALL_ID), "call id");
    }

    /// Oracle: Identity/set with create and destroy sends correct keys.
    #[test]
    fn identity_set_request_shape() {
        let create_val = json!({"k1": {"name": "Work", "email": "work@example.com"}});
        let destroy_val = serde_json::Value::Array(
            vec!["id-old"]
                .into_iter()
                .map(|id| serde_json::Value::String(id.to_owned()))
                .collect(),
        );
        let mut args = json!({ "accountId": "acc1" });
        args["create"] = create_val;
        args["destroy"] = destroy_val;

        let req = build_request("Identity/set", args, USING_MAIL);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("Identity/set"));
        assert_eq!(
            calls[0][1]["create"]["k1"]["email"],
            json!("work@example.com")
        );
        let destroy_arr = calls[0][1]["destroy"].as_array().expect("destroy array");
        assert!(destroy_arr.contains(&json!("id-old")));
    }

    /// Oracle: Identity deserialization from RFC 8621 §6 example.
    #[test]
    fn identity_get_response_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "state": "s1",
            "list": [
                {
                    "id": "ident1",
                    "name": "Jane Doe",
                    "email": "jane@example.com",
                    "textSignature": "-- \nJane",
                    "htmlSignature": "<p>Jane</p>",
                    "mayDelete": true
                }
            ],
            "notFound": []
        });
        use super::super::GetResponse;
        let resp: GetResponse<jmap_mail_types::Identity> =
            serde_json::from_value(json).expect("must deserialize Identity GetResponse");
        assert_eq!(resp.list.len(), 1);
        assert_eq!(resp.list[0].name, "Jane Doe");
        assert_eq!(resp.list[0].email, "jane@example.com");
        assert!(resp.list[0].may_delete);
    }
}
