// JMAP Mail — SearchSnippet/get method implementation on SessionClient.
//
// SearchSnippet/get (RFC 8621 §5) is not a standard /get method: it takes
// `filter` and either `threadIds` or `emailIds` instead of a plain `ids`
// array, and the response shape differs (no `state` field, no `notFound`).
// We therefore return `serde_json::Value` and let the caller deserialize.
//
// Each method follows the standard five-step pattern:
//   1. Validate arguments (defence-in-depth empty-state guards).
//   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//   3. Build args JSON with `serde_json::json!({…})`.
//   4. Call `build_request(method_name, args, USING_MAIL)`.
//   5. Call `self.call_internal(api_url, &req).await?`.
//   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.

use jmap_types::Id;

impl super::SessionClient {
    /// Fetch SearchSnippet objects (RFC 8621 §5 — SearchSnippet/get).
    ///
    /// `filter` is the same filter object used in `Email/query`. Either
    /// `thread_ids` or `email_ids` (or both) may be provided to scope the
    /// snippets; the server returns one [`SearchSnippet`](jmap_mail_types::SearchSnippet) per email in the
    /// result set.
    ///
    /// Returns the raw response value because the SearchSnippet/get response
    /// shape differs from the standard /get shape (no `state`, no `notFound`).
    /// Callers should deserialize into `Vec<jmap_mail_types::SearchSnippet>` via
    /// `response["list"].as_array()`.
    pub async fn search_snippet_get(
        &self,
        account_id: Option<&Id>,
        filter: serde_json::Value,
        thread_ids: Option<&[Id]>,
        email_ids: Option<&[Id]>,
    ) -> Result<serde_json::Value, jmap_base_client::ClientError> {
        let (api_url, session_account_id) = self.session_parts()?;
        let effective_account_id: &str =
            account_id.map(AsRef::as_ref).unwrap_or(session_account_id);
        let mut args = serde_json::json!({
            "accountId": effective_account_id,
            "filter": filter,
        });
        if let Some(tids) = thread_ids {
            args["threadIds"] =
                serde_json::to_value(tids).expect("Id slice Serialize is infallible");
        }
        if let Some(eids) = email_ids {
            args["emailIds"] =
                serde_json::to_value(eids).expect("Id slice Serialize is infallible");
        }
        let req = super::build_request("SearchSnippet/get", args, super::USING_MAIL);
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

    /// Oracle: SearchSnippet/get request shape includes filter and emailIds.
    /// Expected from RFC 8621 §5.
    #[test]
    fn search_snippet_get_request_shape() {
        let args = json!({
            "accountId": "acc1",
            "filter": {"text": "hello"},
            "emailIds": ["e1", "e2"],
        });
        let req = build_request("SearchSnippet/get", args, USING_MAIL);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("SearchSnippet/get"), "method name");
        assert_eq!(calls[0][2], json!(CALL_ID), "call id");
        assert_eq!(calls[0][1]["filter"]["text"], json!("hello"));
        let eids = calls[0][1]["emailIds"].as_array().expect("emailIds array");
        assert_eq!(eids.len(), 2);
    }

    /// Oracle: SearchSnippet/get request with threadIds.
    #[test]
    fn search_snippet_get_with_thread_ids_request_shape() {
        let mut args = json!({
            "accountId": "acc1",
            "filter": {"inMailbox": "mb1"},
        });
        args["threadIds"] = json!(["t1", "t2"]);

        let req = build_request("SearchSnippet/get", args, USING_MAIL);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("SearchSnippet/get"));
        let tids = calls[0][1]["threadIds"]
            .as_array()
            .expect("threadIds array");
        assert!(tids.contains(&json!("t1")));
    }

    // search_snippet_get_empty_email_id_returns_invalid_argument was deleted in
    // JMAP-6by7.2 (typed-Id refactor): under `Option<&[Id]>` the empty-Id case
    // becomes impossible to express through the typed API.

    /// Oracle: SearchSnippet response JSON deserializes into SearchSnippet list.
    /// RFC 8621 §5 example response shape.
    #[test]
    fn search_snippet_response_deserializes() {
        // SearchSnippet/get response uses "accountId" and "list" per RFC 8621 §5.
        let list_json = json!([
            {
                "emailId": "e1",
                "subject": "Hello <mark>world</mark>",
                "preview": "This is a <mark>world</mark>-class message."
            },
            {
                "emailId": "e2"
            }
        ]);
        let snippets: Vec<jmap_mail_types::SearchSnippet> =
            serde_json::from_value(list_json).expect("must deserialize snippet list");
        assert_eq!(snippets.len(), 2);
        assert_eq!(snippets[0].email_id.as_ref(), "e1");
        assert!(snippets[0].subject.is_some());
        assert!(snippets[1].subject.is_none());
    }
}
