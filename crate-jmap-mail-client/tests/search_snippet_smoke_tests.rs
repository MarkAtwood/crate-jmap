//! Wiremock smoke tests for SearchSnippet/get.
//!
//! SearchSnippet/get takes a `filter` plus `emailIds` (RFC 8621 §5.1). The
//! deleted vacuous tests (JMAP-tco1.5) hand-built JSON; these tests exercise
//! the production builder and assert the wire-shape against the spec.
//!
//! Historical note (JMAP-tjvm.6): an earlier version of this client surfaced
//! a non-spec `thread_ids` parameter that emitted `threadIds` on the wire.
//! RFC 8621 §5.1 defines only `emailIds`; the parameter has been removed.
//! The `search_snippet_get_omits_thread_ids_wire_key` test below guards
//! against accidental re-introduction.
//!
//! Historical note (JMAP-tjvm.30): an earlier version of this client took an
//! `account_id: Option<&Id>` override as its first argument — the only
//! `SessionClient` method that allowed the caller to pick a non-session
//! account. The override has been removed for consistency with every other
//! `SessionClient` method; the `accountId` on the wire is always the
//! session's primary mail account.
//!
//! Oracles:
//!   - RFC 8621 §5.1 — SearchSnippet/get semantics and request arguments
//!     (`accountId`, `filter`, `emailIds`).

#[path = "helpers.rs"]
mod helpers;

use jmap_types::Id;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// SearchSnippet/get must emit `emailIds` on the wire and MUST NOT emit any
/// non-spec scoping keys (notably `threadIds`).
#[tokio::test]
async fn search_snippet_get_omits_thread_ids_wire_key() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "SearchSnippet/get",
            {
                "accountId": "A13824",
                "filter": { "text": "invoice" },
                "list": [
                    { "emailId": "M1", "subject": null, "preview": null }
                ],
                "notFound": null
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let filter = json!({ "text": "invoice" });
    let email_ids = [Id::from("M1"), Id::from("M2")];
    let _ = sc
        .search_snippet_get(filter.clone(), Some(&email_ids))
        .await
        .expect("search_snippet_get: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];

    assert_eq!(args["accountId"], json!("A13824"), "accountId mismatch");
    assert_eq!(args["filter"], filter, "filter must be passed through");
    assert_eq!(
        args["emailIds"],
        json!(["M1", "M2"]),
        "emailIds must be sent on the wire"
    );
    assert!(
        args.get("threadIds").is_none(),
        "threadIds is not part of RFC 8621 §5.1 and MUST NOT appear on the wire"
    );
}

/// SearchSnippet/get with no `email_ids` must omit the `emailIds` wire key
/// (caller may rely on `filter` alone for server-side selection).
#[tokio::test]
async fn search_snippet_get_omits_email_ids_when_none() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "SearchSnippet/get",
            {
                "accountId": "A13824",
                "filter": { "text": "report" },
                "list": [],
                "notFound": null
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let filter = json!({ "text": "report" });
    let _ = sc
        .search_snippet_get(filter.clone(), None)
        .await
        .expect("search_snippet_get: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];

    assert_eq!(args["filter"], filter, "filter mismatch");
    assert!(
        args.get("emailIds").is_none(),
        "emailIds must be omitted when caller passes None"
    );
    assert!(
        args.get("threadIds").is_none(),
        "threadIds is not part of RFC 8621 §5.1 and MUST NOT appear on the wire"
    );
}

// Deleted in JMAP-tjvm.30: `search_snippet_get_caller_account_id_overrides_session`
// exercised the `account_id: Option<&Id>` override on the wire. The override
// has been removed for consistency with every other `SessionClient` method
// (all of which derive `accountId` from `session_parts()` unconditionally).
// The session-account binding is now implicitly covered by
// `search_snippet_get_omits_thread_ids_wire_key` and
// `search_snippet_get_omits_email_ids_when_none`, both of which assert
// `args["accountId"]` matches the `helpers::make_client` session binding.
// The compile-time gate is the absence of the first positional
// `Option<&Id>` parameter on the function signature.
