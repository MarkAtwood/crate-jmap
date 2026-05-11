//! Wiremock smoke tests for SearchSnippet/get.
//!
//! SearchSnippet/get takes a `filter` plus either `threadIds` OR `emailIds`
//! (or both — RFC 8621 §6 allows both, with the server returning snippets
//! for any matching email in the union). The deleted vacuous tests
//! (JMAP-tco1.5) hand-built JSON; these tests exercise the production builder
//! and assert the wire-shape distinction between the two scoping modes.
//!
//! Oracles:
//!   - RFC 8621 §6 — SearchSnippet/get semantics
//!   - RFC 8621 §6.1 — request arguments: `accountId`, `filter`, `emailIds`,
//!     and (via the SearchSnippet/get spec text) `threadIds` as a scoping
//!     helper that expands to all emails in the listed threads.
//!   - RFC 8620 §3.1 — `accountId` may be overridden by caller-supplied value.

#[path = "helpers.rs"]
mod helpers;

use jmap_types::Id;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// SearchSnippet/get scoped by `email_ids` only must emit `emailIds` on the
/// wire and MUST NOT emit `threadIds`.
#[tokio::test]
async fn search_snippet_get_email_ids_only() {
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
        .search_snippet_get(None, filter.clone(), None, Some(&email_ids))
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
        "threadIds must be omitted when caller passes None"
    );
}

/// SearchSnippet/get scoped by `thread_ids` only must emit `threadIds` on the
/// wire and MUST NOT emit `emailIds`. This distinguishes thread-level scoping
/// (find all snippet matches across emails in the listed threads) from
/// email-level scoping (find matches only in the listed emails).
#[tokio::test]
async fn search_snippet_get_thread_ids_only() {
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
    let thread_ids = [Id::from("T-A"), Id::from("T-B")];
    let _ = sc
        .search_snippet_get(None, filter.clone(), Some(&thread_ids), None)
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
    assert_eq!(
        args["threadIds"],
        json!(["T-A", "T-B"]),
        "threadIds must be sent on the wire"
    );
    assert!(
        args.get("emailIds").is_none(),
        "emailIds must be omitted when caller passes None"
    );
}

/// SearchSnippet/get with BOTH `thread_ids` and `email_ids` must emit BOTH
/// wire keys (RFC 8621 §6 permits both — the server returns snippets for the
/// union of matching emails).
#[tokio::test]
async fn search_snippet_get_both_scoping_modes() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "SearchSnippet/get",
            {
                "accountId": "A13824",
                "filter": { "text": "q3" },
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
    let filter = json!({ "text": "q3" });
    let thread_ids = [Id::from("T1")];
    let email_ids = [Id::from("M9")];
    let _ = sc
        .search_snippet_get(None, filter.clone(), Some(&thread_ids), Some(&email_ids))
        .await
        .expect("search_snippet_get: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];

    assert_eq!(args["threadIds"], json!(["T1"]), "threadIds mismatch");
    assert_eq!(args["emailIds"], json!(["M9"]), "emailIds mismatch");
}

/// SearchSnippet/get with a caller-supplied `account_id` must override the
/// session's primary account on the wire (RFC 8620 §3.1 — caller may pin
/// a specific account for cross-account snippet lookups).
#[tokio::test]
async fn search_snippet_get_caller_account_id_overrides_session() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "SearchSnippet/get",
            {
                "accountId": "B99999",
                "filter": { "text": "x" },
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
    let other = Id::from("B99999");
    let filter = json!({ "text": "x" });
    let email_ids = [Id::from("M1")];
    let _ = sc
        .search_snippet_get(Some(&other), filter, None, Some(&email_ids))
        .await
        .expect("search_snippet_get: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];

    assert_eq!(
        args["accountId"],
        json!("B99999"),
        "caller-supplied accountId must override session primary"
    );
}
