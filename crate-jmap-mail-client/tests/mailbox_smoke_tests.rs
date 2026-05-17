//! Wiremock smoke tests for Mailbox/set and Mailbox/query.
//!
//! Focus: production-path wire-shape assertions for arguments that previously
//! had only vacuous build_request tests (deleted in JMAP-tco1.5 / JMAP-231o.8).
//! Each test mounts a wiremock server, invokes the production builder via
//! `SessionClient`, then asserts on the recorded request body.
//!
//! Oracles:
//!   - Mailbox/set: RFC 8621 §2.5 (Mailbox/set, `onDestroyRemoveEmails`)
//!   - Mailbox/query: RFC 8621 §2.3 (filter conditions: `parentId`, `name`,
//!     `role`, `hasAnyRole`, `isSubscribed`)
//!   - JMAP batch request envelope: RFC 8620 §3.3

#[path = "helpers.rs"]
mod helpers;

use jmap_mail_client::MailboxSetParams;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Mailbox/set with `MailboxSetParams { on_destroy_remove_emails: Some(true) }`
/// must send `onDestroyRemoveEmails: true` as a top-level wire argument
/// (RFC 8621 §2.5 — required to destroy a non-empty Mailbox).
#[tokio::test]
async fn mailbox_set_on_destroy_remove_emails_passthrough() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Mailbox/set",
            {
                "accountId": "A13824",
                "oldState": "m1",
                "newState": "m2",
                "destroyed": ["MB-old"],
                "notDestroyed": null
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
    let params = MailboxSetParams {
        on_destroy_remove_emails: Some(true),
        extra: serde_json::Map::new(),
    };
    let _ = sc
        .mailbox_set(
            None,
            None,
            Some(vec![jmap_types::Id::from("MB-old")]),
            Some(params),
        )
        .await
        .expect("mailbox_set: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];

    assert_eq!(args["accountId"], json!("A13824"), "accountId mismatch");
    assert_eq!(
        args["onDestroyRemoveEmails"],
        json!(true),
        "onDestroyRemoveEmails must be true on the wire"
    );
    assert_eq!(
        args["destroy"],
        json!(["MB-old"]),
        "destroy ids must be passed through"
    );
    // Caller passed None for both create and update, so those keys must be omitted.
    assert!(
        args.get("create").is_none(),
        "create must be omitted when caller passes None"
    );
    assert!(
        args.get("update").is_none(),
        "update must be omitted when caller passes None"
    );
}

/// Mailbox/set called without `MailboxSetParams` must NOT emit
/// `onDestroyRemoveEmails` on the wire (the field is optional in RFC 8621 §2.5
/// and defaults to false server-side; emitting it as null would create a wire
/// divergence from the spec example).
#[tokio::test]
async fn mailbox_set_without_params_omits_on_destroy_remove_emails() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Mailbox/set",
            {
                "accountId": "A13824",
                "oldState": "m1",
                "newState": "m1",
                "created": null,
                "updated": null,
                "destroyed": null
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
    // Need at least one of create/update/destroy to be Some — the
    // all-None /set is rejected client-side by the defence-in-depth
    // guard (bd:JMAP-tjvm.24). An empty destroy slice is the smallest
    // valid input; the test's actual oracle is the *absence* of
    // onDestroyRemoveEmails from the wire when params is None, which
    // still holds when destroy is Some.
    let destroy_ids: Vec<jmap_types::Id> = vec![];
    let _ = sc
        .mailbox_set(None, None, Some(destroy_ids), None)
        .await
        .expect("mailbox_set: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];

    assert!(
        args.get("onDestroyRemoveEmails").is_none(),
        "onDestroyRemoveEmails must be omitted when params is None"
    );
}

/// Mailbox/query with a non-trivial filter object must pass it through verbatim.
///
/// Oracle: RFC 8621 §2.3 — Mailbox/query supports a FilterCondition with
/// `parentId`, `name`, `role`, `hasAnyRole`, `isSubscribed`. The client takes
/// `filter: Option<serde_json::Value>` (untyped at the boundary so callers can
/// also build FilterOperator trees) so the body assertion targets the exact
/// JSON value the caller built.
#[tokio::test]
async fn mailbox_query_filter_passthrough() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Mailbox/query",
            {
                "accountId": "A13824",
                "queryState": "qs1",
                "canCalculateChanges": true,
                "position": 0,
                "ids": ["MB-inbox"],
                "total": null,
                "limit": null
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
    let filter = json!({
        "operator": "AND",
        "conditions": [
            { "role": "inbox" },
            { "isSubscribed": true }
        ]
    });
    let sort = json!([
        { "property": "name", "isAscending": true }
    ]);
    let _ = sc
        .mailbox_query(Some(filter.clone()), Some(sort.clone()), Some(0), Some(50))
        .await
        .expect("mailbox_query: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];

    assert_eq!(args["accountId"], json!("A13824"), "accountId mismatch");
    assert_eq!(
        args["filter"], filter,
        "filter must be passed through verbatim"
    );
    assert_eq!(args["sort"], sort, "sort must be passed through verbatim");
    assert_eq!(args["position"], json!(0), "position mismatch");
    assert_eq!(args["limit"], json!(50), "limit mismatch");
}

/// Mailbox/query with no filter/sort/position/limit must omit those keys
/// (RFC 8620 §5.5 — only required arg is `accountId`).
#[tokio::test]
async fn mailbox_query_no_args_omits_optional_keys() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Mailbox/query",
            {
                "accountId": "A13824",
                "queryState": "qs0",
                "canCalculateChanges": false,
                "position": 0,
                "ids": []
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
    let _ = sc
        .mailbox_query(None, None, None, None)
        .await
        .expect("mailbox_query: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];

    assert_eq!(args["accountId"], json!("A13824"), "accountId mismatch");
    assert!(args.get("filter").is_none(), "filter must be omitted");
    assert!(args.get("sort").is_none(), "sort must be omitted");
    assert!(args.get("position").is_none(), "position must be omitted");
    assert!(args.get("limit").is_none(), "limit must be omitted");
}
