// JMAP Mail — Email/* method implementations on SessionClient.
//
// Each method follows the standard five-step pattern:
//   1. Validate arguments (defence-in-depth empty-state guards).
//   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//   3. Build args JSON with `serde_json::json!({…})`.
//   4. Call `build_request(method_name, args, USING_MAIL)`.
//   5. Call `self.call_internal(api_url, &req).await?`.
//   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.

use std::collections::HashMap;

use jmap_types::{Id, PatchObject, State};

use super::{
    ChangesResponse, EmailCopyParams, EmailGetParams, GetResponse, QueryChangesResponse,
    QueryResponse, SetResponse,
};

impl super::SessionClient {
    /// Fetch Email objects by IDs (RFC 8621 §4.1.8 — Email/get).
    ///
    /// If `ids` is `None`, the server returns all Emails for the account.
    /// Pass `properties: None` to return all fields.
    /// Pass `params: None` to use server defaults for body-fetch options.
    pub async fn email_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
        params: Option<EmailGetParams>,
    ) -> Result<GetResponse<jmap_mail_types::Email>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "ids": ids,
            "properties": properties,
        });
        if let Some(p) = params {
            let pv = serde_json::to_value(p).map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "email_get: failed to serialize params: {e}"
                ))
            })?;
            if let serde_json::Value::Object(map) = pv {
                for (k, v) in map {
                    args[k] = v;
                }
            }
        }
        let req = super::build_request("Email/get", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to Email objects since `since_state` (RFC 8621 §4.2 — Email/changes).
    ///
    /// If `has_more_changes` is true in the response, call again with `new_state`
    /// as `since_state` until the flag is false.
    pub async fn email_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `thread_changes`.
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "email_changes: since_state may not be empty".into(),
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
        let req = super::build_request("Email/changes", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create, update, or destroy Email objects (RFC 8621 §4.3 — Email/set).
    ///
    /// Pass `create`, `update`, and/or `destroy` as needed. All three are
    /// optional; pass `None` to omit any operation from the request.
    /// Pass `if_in_state: Some(&state)` to use an optimistic-concurrency guard.
    ///
    /// `update` is `Option<HashMap<Id, PatchObject>>` (RFC 8620 §5.3). Wire
    /// format is unchanged from a plain JSON object because [`PatchObject`]
    /// is `#[serde(transparent)]`; the typed parameter binds the JSON Pointer
    /// key + null-leaf removal contract to the type system.
    pub async fn email_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<HashMap<Id, PatchObject>>,
        destroy: Option<Vec<Id>>,
        if_in_state: Option<&State>,
    ) -> Result<SetResponse<jmap_mail_types::Email>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
        });
        if let Some(s) = if_in_state {
            args["ifInState"] = s.as_ref().into();
        }
        if let Some(c) = create {
            args["create"] = c;
        }
        if let Some(u) = update {
            args["update"] = serde_json::to_value(&u).map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "email_set: serializing update map failed: {e}"
                ))
            })?;
        }
        if let Some(d) = destroy {
            args["destroy"] = serde_json::to_value(&d).expect("Id Vec Serialize is infallible");
        }
        let req = super::build_request("Email/set", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Query Email IDs with optional filter and sort (RFC 8621 §4.4 — Email/query).
    ///
    /// Pass `filter: None` and `sort: None` to return all Emails with
    /// server-default ordering. Use `position` and `limit` for pagination.
    /// Pass `collapse_threads: Some(true)` to return at most one email per thread.
    pub async fn email_query(
        &self,
        filter: Option<serde_json::Value>,
        sort: Option<serde_json::Value>,
        position: Option<u64>,
        limit: Option<u64>,
        collapse_threads: Option<bool>,
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
        if let Some(ct) = collapse_threads {
            args["collapseThreads"] = ct.into();
        }
        let req = super::build_request("Email/query", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch query-result changes for Email since `since_query_state`
    /// (RFC 8621 §4.5 — Email/queryChanges).
    pub async fn email_query_changes(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
        collapse_threads: Option<bool>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `thread_changes`.
        if since_query_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "email_query_changes: since_query_state may not be empty".into(),
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
        if let Some(ct) = collapse_threads {
            args["collapseThreads"] = ct.into();
        }
        let req = super::build_request("Email/queryChanges", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Copy Emails from another account (RFC 8621 §4.7 — Email/copy).
    ///
    /// `params` carries `fromAccountId` and optional destroy-after-copy flags.
    /// `create` is a map of creation keys to partial Email objects (with new
    /// mailboxIds etc.) as described in RFC 8621 §4.7.
    pub async fn email_copy(
        &self,
        params: EmailCopyParams,
        create: serde_json::Value,
    ) -> Result<SetResponse<jmap_mail_types::Email>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "fromAccountId": params.from_account_id,
            "create": create,
        });
        if let Some(v) = params.on_success_destroy_original {
            args["onSuccessDestroyOriginal"] = v.into();
        }
        if let Some(v) = params.destroy_from_if_in_state {
            args["destroyFromIfInState"] = v.as_ref().into();
        }
        let req = super::build_request("Email/copy", args, super::USING_MAIL);
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

    // email_get_empty_id_returns_invalid_argument was deleted in JMAP-6by7.2
    // (typed-Id refactor): under `Option<&[Id]>` the empty-Id case becomes
    // impossible to express through the typed API.

    // The InvalidArgument guards for empty since_state and since_query_state
    // live in email_changes / email_query_changes production code; testing them
    // requires a wiremock-backed async harness. See JMAP-sc1b.64.

    /// Oracle: Email/get request has correct method name and using array.
    /// Expected JSON shape from RFC 8620 §3.3.
    #[test]
    fn email_get_request_shape() {
        let args = json!({
            "accountId": "acc1",
            "ids": ["e1", "e2"],
            "properties": ["id", "subject"],
        });
        let req = build_request("Email/get", args, USING_MAIL);
        let v = serde_json::to_value(&req).expect("serialize");

        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("Email/get"), "method name");
        assert_eq!(calls[0][2], json!(CALL_ID), "call id");

        let using = v["using"].as_array().expect("using");
        assert!(using.contains(&json!("urn:ietf:params:jmap:mail")));
        assert!(using.contains(&json!("urn:ietf:params:jmap:core")));
    }

    /// Oracle: Email/changes request includes sinceState in args.
    #[test]
    fn email_changes_request_includes_since_state() {
        let args = json!({
            "accountId": "acc1",
            "sinceState": "state42",
        });
        let req = build_request("Email/changes", args, USING_MAIL);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][1]["sinceState"], json!("state42"));
    }

    /// Oracle: Email/set with destroy list sends destroy array in args.
    #[test]
    fn email_set_destroy_request_shape() {
        let destroy_ids = ["e1", "e2"];
        let destroy_val = serde_json::Value::Array(
            destroy_ids
                .iter()
                .copied()
                .map(serde_json::Value::from)
                .collect(),
        );
        let mut args = json!({ "accountId": "acc1" });
        args["destroy"] = destroy_val;

        let req = build_request("Email/set", args, USING_MAIL);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("Email/set"));
        let destroy_arr = calls[0][1]["destroy"].as_array().expect("destroy array");
        assert_eq!(destroy_arr.len(), 2);
        assert!(destroy_arr.contains(&json!("e1")));
        assert!(destroy_arr.contains(&json!("e2")));
    }

    /// Oracle: Email/copy request carries fromAccountId.
    /// Expected from RFC 8621 §4.7.
    #[test]
    fn email_copy_request_shape() {
        let args = json!({
            "accountId": "acc-dest",
            "fromAccountId": "acc-src",
            "create": { "k1": { "id": "e1" } },
            "onSuccessDestroyOriginal": true,
        });
        let req = build_request("Email/copy", args, USING_MAIL);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("Email/copy"), "method name");
        assert_eq!(calls[0][1]["fromAccountId"], json!("acc-src"));
        assert_eq!(calls[0][1]["onSuccessDestroyOriginal"], json!(true));
    }

    /// Oracle: Email/query request with collapseThreads.
    #[test]
    fn email_query_request_shape() {
        let mut args = json!({ "accountId": "acc1" });
        args["collapseThreads"] = json!(true);
        args["filter"] = json!({"inMailbox": "mb1"});

        let req = build_request("Email/query", args, USING_MAIL);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("Email/query"));
        assert_eq!(calls[0][1]["collapseThreads"], json!(true));
        assert_eq!(calls[0][1]["filter"]["inMailbox"], json!("mb1"));
    }

    /// Oracle: Email deserialization from RFC 8621 §4 example JSON subset.
    /// Only fields present in the fixture are checked; Email has many optional fields.
    #[test]
    fn email_get_response_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "state": "s10",
            "list": [
                {
                    "id": "e1",
                    "blobId": "b1",
                    "threadId": "t1",
                    "mailboxIds": { "mb1": true },
                    "keywords": { "$seen": true },
                    "size": 1024,
                    "receivedAt": "2024-01-01T00:00:00Z"
                }
            ],
            "notFound": []
        });
        use super::super::GetResponse;
        let resp: GetResponse<jmap_mail_types::Email> =
            serde_json::from_value(json).expect("must deserialize Email GetResponse");
        assert_eq!(resp.list.len(), 1);
        assert_eq!(resp.list[0].id.as_ref(), "e1");
    }
}
