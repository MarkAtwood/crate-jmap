//! JMAP Mail — SearchSnippet/get method implementation on SessionClient.
//!
//! SearchSnippet/get (RFC 8621 §5.1) is not a standard /get method: it takes
//! `filter` and `emailIds` instead of a plain `ids` array, and the response
//! shape differs (no `state` field, but does include `notFound`). We
//! therefore return `serde_json::Value` and let the caller deserialize.
//!
//! Each method follows the standard five-step pattern:
//!   1. Validate arguments (defence-in-depth empty-state guards).
//!   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//!   3. Build args JSON with `serde_json::json!({…})`.
//!   4. Call `build_request(method_name, args, USING_MAIL)`.
//!   5. Call `self.call_internal(api_url, &req).await?`.
//!   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.

use jmap_types::Id;

impl super::SessionClient {
    /// Fetch SearchSnippet objects (RFC 8621 §5.1 — SearchSnippet/get).
    ///
    /// `filter` is the same filter object used in `Email/query`. `email_ids`
    /// is the spec-defined list of Email ids to fetch snippets for; the
    /// server returns one [`SearchSnippet`](jmap_mail_types::SearchSnippet)
    /// per email in the result set.
    ///
    /// Callers wishing to fetch snippets for all emails in a set of threads
    /// must resolve the thread membership first via `Thread/get`, then pass
    /// the resulting email ids here — RFC 8621 §5.1 does not define a
    /// `threadIds` argument on this method.
    ///
    /// Returns the raw response value because the SearchSnippet/get response
    /// shape differs from the standard /get shape (no `state`).
    /// Callers should deserialize into `Vec<jmap_mail_types::SearchSnippet>` via
    /// `response["list"].as_array()`.
    ///
    /// The account is the one bound to this [`SessionClient`](super::SessionClient)
    /// — there is no caller-supplied override (closed bd:JMAP-tjvm.30 for
    /// consistency with every other `SessionClient` method, which all derive
    /// `accountId` from the session unconditionally).
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:mail`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call):
    ///   [`Http`](jmap_base_client::ClientError::Http),
    ///   [`Parse`](jmap_base_client::ClientError::Parse),
    ///   [`AuthFailed`](jmap_base_client::ClientError::AuthFailed),
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError)
    ///   (wraps RFC 8620 §3.6.2 method-level errors such as
    ///   `accountNotFound`, `invalidArguments`, `serverFail`, and the
    ///   /query-shape errors `unsupportedFilter` / `unsupportedSort`
    ///   per RFC 8621 §5.1),
    ///   [`MethodNotFound`](jmap_base_client::ClientError::MethodNotFound),
    ///   [`ResponseTooLarge`](jmap_base_client::ClientError::ResponseTooLarge),
    ///   or
    ///   [`UnexpectedResponse`](jmap_base_client::ClientError::UnexpectedResponse).
    pub async fn search_snippet_get(
        &self,
        filter: serde_json::Value,
        email_ids: Option<&[Id]>,
    ) -> Result<serde_json::Value, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "filter": filter,
        });
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
    use serde_json::json;

    // search_snippet_get_empty_email_id_returns_invalid_argument was deleted in
    // JMAP-6by7.2 (typed-Id refactor): under `Option<&[Id]>` the empty-Id case
    // becomes impossible to express through the typed API.

    // Deleted in JMAP-tco1.5 as Pattern E (vacuous inline tests):
    //   - search_snippet_get_request_shape
    //   - search_snippet_get_with_thread_ids_request_shape
    // Each hand-built `args = json!({...})` and fed it to `build_request`,
    // never invoking the `search_snippet_get` production builder. Real
    // production-path coverage for this method is provided by the wiremock
    // tests in `tests/search_snippet_smoke_tests.rs`.
    //
    // The non-spec `thread_ids` parameter that those deleted tests covered
    // was itself removed in JMAP-tjvm.6: RFC 8621 §5.1 defines only
    // `emailIds` on SearchSnippet/get.
    //
    // `build_request`, `CALL_ID`, and `USING_MAIL` themselves have their
    // own focused tests in `methods/mod.rs`.

    /// Oracle: SearchSnippet response JSON deserializes into SearchSnippet list.
    /// RFC 8621 §5.1 example response shape.
    #[test]
    fn search_snippet_response_deserializes() {
        // SearchSnippet/get response uses "accountId" and "list" per RFC 8621 §5.1.
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
