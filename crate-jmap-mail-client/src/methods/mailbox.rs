// JMAP Mail — Mailbox/* method implementations on SessionClient.
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
    ChangesResponse, GetResponse, MailboxSetParams, QueryChangesResponse, QueryResponse,
    SetResponse,
};

impl super::SessionClient {
    /// Fetch Mailbox objects by IDs (RFC 8621 §2.1 — Mailbox/get).
    ///
    /// If `ids` is `None`, the server returns all Mailboxes for the account.
    /// Pass `properties: None` to return all fields.
    pub async fn mailbox_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_mail_types::Mailbox>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let args = serde_json::json!({
            "accountId": account_id,
            "ids": ids,
            "properties": properties,
        });
        let req = super::build_request("Mailbox/get", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to Mailbox objects since `since_state` (RFC 8621 §2.2 — Mailbox/changes).
    pub async fn mailbox_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `thread_changes`.
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "mailbox_changes: since_state may not be empty".into(),
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
        let req = super::build_request("Mailbox/changes", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create, update, or destroy Mailbox objects (RFC 8621 §2.5 — Mailbox/set).
    ///
    /// Pass `create`, `update`, and/or `destroy` as needed. Pass `None` to omit.
    /// Pass `params: Some(MailboxSetParams { on_destroy_remove_emails: Some(true) })`
    /// to allow destroying a non-empty mailbox.
    ///
    /// `update` is `Option<HashMap<Id, PatchObject>>` (RFC 8620 §5.3). Wire
    /// format is unchanged from a plain JSON object because [`PatchObject`]
    /// is `#[serde(transparent)]`; the typed parameter binds the JSON Pointer
    /// key + null-leaf removal contract to the type system.
    pub async fn mailbox_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<HashMap<Id, PatchObject>>,
        destroy: Option<Vec<Id>>,
        params: Option<MailboxSetParams>,
    ) -> Result<SetResponse<jmap_mail_types::Mailbox>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
        });
        if let Some(p) = params {
            if let Some(v) = p.on_destroy_remove_emails {
                args["onDestroyRemoveEmails"] = v.into();
            }
        }
        if let Some(c) = create {
            args["create"] = c;
        }
        if let Some(u) = update {
            args["update"] = serde_json::to_value(&u).map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "mailbox_set: serializing update map failed: {e}"
                ))
            })?;
        }
        if let Some(d) = destroy {
            args["destroy"] = serde_json::to_value(&d).expect("Id Vec Serialize is infallible");
        }
        let req = super::build_request("Mailbox/set", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Query Mailbox IDs with optional filter and sort (RFC 8621 §2.3 — Mailbox/query).
    ///
    /// Pass `filter: None` and `sort: None` to return all Mailboxes with
    /// server-default ordering. Use `position` and `limit` for pagination.
    pub async fn mailbox_query(
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
        let req = super::build_request("Mailbox/query", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch query-result changes for Mailbox since `since_query_state`
    /// (RFC 8621 §2.4 — Mailbox/queryChanges).
    pub async fn mailbox_query_changes(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `thread_changes`.
        if since_query_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "mailbox_query_changes: since_query_state may not be empty".into(),
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
        let req = super::build_request("Mailbox/queryChanges", args, super::USING_MAIL);
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

    // mailbox_get_empty_id_returns_invalid_argument was deleted in JMAP-6by7.2
    // (typed-Id refactor): under `Option<&[Id]>` the empty-Id case becomes
    // impossible to express through the typed API.

    // The InvalidArgument guard for empty since_state lives in mailbox_changes
    // production code; testing it requires a wiremock-backed async harness.
    // See JMAP-sc1b.64.

    /// Oracle: Mailbox/get request has correct method name.
    #[test]
    fn mailbox_get_request_shape() {
        let args = json!({
            "accountId": "acc1",
            "ids": null,
            "properties": null,
        });
        let req = build_request("Mailbox/get", args, USING_MAIL);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("Mailbox/get"), "method name");
        assert_eq!(calls[0][2], json!(CALL_ID), "call id");
    }

    /// Oracle: Mailbox/set with onDestroyRemoveEmails in args.
    /// Expected key name from RFC 8621 §2.5.
    #[test]
    fn mailbox_set_on_destroy_remove_emails_request_shape() {
        let mut args = json!({ "accountId": "acc1" });
        args["onDestroyRemoveEmails"] = json!(true);
        args["destroy"] = json!(["mb1"]);

        let req = build_request("Mailbox/set", args, USING_MAIL);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("Mailbox/set"));
        assert_eq!(calls[0][1]["onDestroyRemoveEmails"], json!(true));
        let destroy_arr = calls[0][1]["destroy"].as_array().expect("destroy array");
        assert_eq!(destroy_arr.len(), 1);
    }

    /// Oracle: Mailbox/query with filter sends filter in args.
    #[test]
    fn mailbox_query_request_includes_filter() {
        let filter = json!({"role": "inbox"});
        let mut args = json!({ "accountId": "acc1" });
        args["filter"] = filter;

        let req = build_request("Mailbox/query", args, USING_MAIL);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][1]["filter"]["role"], json!("inbox"));
    }

    // The InvalidArgument guard for empty since_query_state lives in
    // mailbox_query_changes production code; testing it requires a
    // wiremock-backed async harness. See JMAP-sc1b.64.

    /// Oracle: Mailbox deserialization from RFC 8621 §2 example.
    #[test]
    fn mailbox_get_response_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "state": "s10",
            "list": [
                {
                    "id": "mb1",
                    "name": "Inbox",
                    "role": "inbox",
                    "sortOrder": 10,
                    "totalEmails": 42,
                    "unreadEmails": 3,
                    "totalThreads": 20,
                    "unreadThreads": 2,
                    "myRights": {
                        "mayReadItems": true,
                        "mayAddItems": true,
                        "mayRemoveItems": true,
                        "maySetSeen": true,
                        "maySetKeywords": true,
                        "mayCreateChild": true,
                        "mayRename": true,
                        "mayDelete": false,
                        "maySubmit": false
                    },
                    "isSubscribed": true
                }
            ],
            "notFound": []
        });
        use super::super::GetResponse;
        let resp: GetResponse<jmap_mail_types::Mailbox> =
            serde_json::from_value(json).expect("must deserialize Mailbox GetResponse");
        assert_eq!(resp.list.len(), 1);
        assert_eq!(resp.list[0].name, "Inbox");
    }
}
