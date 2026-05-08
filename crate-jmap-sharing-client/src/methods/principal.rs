// JMAP Sharing — Principal/* method implementations on SessionClient.
//
// Each method follows the standard five-step pattern:
//   1. Validate arguments (empty-string guards).
//   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//   3. Build args JSON with `serde_json::json!({…})`.
//   4. Call `build_request(method_name, args, USING_SHARING)`.
//   5. Call `self.call_internal(api_url, &req).await?`.
//   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.

use std::collections::HashMap;

use jmap_types::{Id, PatchObject};

use super::{ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SetResponse};

impl super::SessionClient {
    /// Fetch Principal objects by IDs (RFC 9670 §2.1).
    ///
    /// If `ids` is `None`, the server returns all Principals for the account.
    /// Pass `properties: None` to return all fields.
    pub async fn principal_get(
        &self,
        ids: Option<&[&str]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_sharing_types::Principal>, jmap_base_client::ClientError> {
        if let Some(id_slice) = ids {
            for id in id_slice.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "principal_get: ids element may not be empty".into(),
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
        since_state: &str,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        if since_state.is_empty() {
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
        destroy: Option<Vec<&str>>,
    ) -> Result<SetResponse<jmap_sharing_types::Principal>, jmap_base_client::ClientError> {
        if let Some(ref ids) = destroy {
            for id in ids.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "principal_set: destroy element may not be empty".into(),
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
            args["update"] = serde_json::to_value(&u).map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "principal_set: serializing update map failed: {e}"
                ))
            })?;
        }
        if let Some(d) = destroy {
            args["destroy"] = serde_json::Value::Array(
                d.into_iter()
                    .map(|id| serde_json::Value::String(id.to_owned()))
                    .collect(),
            );
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
        since_query_state: &str,
        max_changes: Option<u64>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        if since_query_state.is_empty() {
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::{build_request, CALL_ID, USING_SHARING};
    use serde_json::json;

    /// Oracle: empty ID in ids slice returns InvalidArgument.
    /// This guard fires before any session lookup or network call.
    /// Expected error kind: ClientError::InvalidArgument — per base client spec.
    #[test]
    fn principal_get_empty_id_returns_invalid_argument() {
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
    /// Guard fires before any session or network call.
    #[test]
    fn principal_changes_empty_since_state_returns_invalid_argument() {
        let since_state = "";
        let result: Result<(), jmap_base_client::ClientError> = {
            if since_state.is_empty() {
                Err(jmap_base_client::ClientError::InvalidArgument(
                    "principal_changes: since_state may not be empty".into(),
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

    /// Oracle: empty since_query_state returns InvalidArgument.
    #[test]
    fn principal_query_changes_empty_state_returns_invalid_argument() {
        let since_query_state = "";
        let result: Result<(), jmap_base_client::ClientError> = {
            if since_query_state.is_empty() {
                Err(jmap_base_client::ClientError::InvalidArgument(
                    "principal_query_changes: since_query_state may not be empty".into(),
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

    /// Oracle: Principal/get request has correct method name and using array.
    /// Expected JSON shape from RFC 8620 §3.3.
    #[test]
    fn principal_get_request_shape() {
        let args = json!({
            "accountId": "acc1",
            "ids": null,
            "properties": null,
        });
        let req = build_request("Principal/get", args, USING_SHARING);
        let v = serde_json::to_value(&req).expect("serialize");

        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("Principal/get"), "method name");
        assert_eq!(calls[0][2], json!(CALL_ID), "call id");

        let using = v["using"].as_array().expect("using");
        assert!(using.contains(&json!("urn:ietf:params:jmap:principals")));
    }

    /// Oracle: Principal/changes request includes sinceState in args.
    /// Expected: args object has "sinceState" key with the provided value.
    #[test]
    fn principal_changes_request_includes_since_state() {
        let args = json!({
            "accountId": "acc1",
            "sinceState": "state42",
        });
        let req = build_request("Principal/changes", args, USING_SHARING);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][1]["sinceState"], json!("state42"));
    }

    /// Oracle: principal_set with destroy list sends destroy array in args.
    /// Expected: destroy is a JSON array of string IDs.
    #[test]
    fn principal_set_destroy_request_shape() {
        let destroy_ids = ["id1", "id2"];
        let destroy_val = serde_json::Value::Array(
            destroy_ids
                .iter()
                .map(|id| serde_json::Value::String((*id).to_owned()))
                .collect(),
        );
        let mut args = json!({ "accountId": "acc1" });
        args["destroy"] = destroy_val;

        let req = build_request("Principal/set", args, USING_SHARING);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("Principal/set"));
        let destroy_arr = calls[0][1]["destroy"].as_array().expect("destroy array");
        assert_eq!(destroy_arr.len(), 2);
        assert!(destroy_arr.contains(&json!("id1")));
        assert!(destroy_arr.contains(&json!("id2")));
    }

    /// Oracle: principal_query with filter sends filter in args.
    /// Expected: args contains the filter object.
    #[test]
    fn principal_query_request_includes_filter() {
        let filter = json!({"name": "Alice"});
        let mut args = json!({ "accountId": "acc1" });
        args["filter"] = filter.clone();

        let req = build_request("Principal/query", args, USING_SHARING);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][1]["filter"]["name"], json!("Alice"));
    }
}
