//! Regression tests for JMAP-tjvm.1 / JMAP-tjvm.2: callers MUST NOT be able
//! to override typed wire fields (accountId, ids, properties, blobIds) by
//! putting those keys into `EmailGetParams.extra` / `EmailParseParams.extra`.
//!
//! The workspace extras-preservation policy (see workspace AGENTS.md) is
//! that `extra` carries vendor / site / private extension fields losslessly
//! across the wire. It is NOT a back-door for callers to subvert the typed
//! API. These tests pin the precedence: typed wins on collision.
//!
//! Oracles:
//!   - RFC 8621 §4.2 — Email/get accountId is canonical for the account
//!   - RFC 8621 §4.9 — Email/parse blobIds is canonical for the blob set
//!   - Workspace AGENTS.md "Extras-preservation policy for vendor/site fields"

#[path = "helpers.rs"]
mod helpers;

use jmap_mail_client::methods::{EmailGetParams, EmailParseParams};
use jmap_types::Id;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Email/get: a caller who puts `accountId` in `params.extra` MUST NOT be
/// able to override the accountId computed from the bound session (or
/// from the caller-supplied account_id argument).
#[tokio::test]
async fn email_get_extras_cannot_override_account_id() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Email/get",
            { "accountId": "A13824", "state": "s5", "list": [], "notFound": [] },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    // Adversarial caller: try to override accountId and ids via extras.
    let mut extra = serde_json::Map::new();
    extra.insert("accountId".to_owned(), json!("VICTIM"));
    extra.insert("ids".to_owned(), json!(["malicious-id"]));
    let params = EmailGetParams {
        body_properties: None,
        fetch_text_body_values: None,
        fetch_html_body_values: None,
        fetch_all_body_values: None,
        max_body_value_bytes: None,
        extra,
    };

    let ids = [Id::from("legit-1")];
    sc.email_get(Some(&ids), None, Some(params))
        .await
        .expect("email_get: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be JSON");
    let args = &body["methodCalls"][0][1];

    // Typed accountId (from session, since None was passed) must win.
    assert_eq!(
        args["accountId"],
        json!("A13824"),
        "extras MUST NOT override accountId; got {args:?}"
    );
    // Typed ids must win over extras["ids"].
    assert_eq!(
        args["ids"],
        json!(["legit-1"]),
        "extras MUST NOT override ids; got {args:?}"
    );
}

/// Email/parse: a caller who puts `accountId` / `blobIds` in `params.extra`
/// MUST NOT be able to override the typed values.
#[tokio::test]
async fn email_parse_extras_cannot_override_account_or_blobs() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Email/parse",
            { "accountId": "A13824", "parsed": {}, "notParsable": [], "notFound": [] },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let mut extra = serde_json::Map::new();
    extra.insert("accountId".to_owned(), json!("VICTIM"));
    extra.insert("blobIds".to_owned(), json!(["malicious-blob"]));
    let params = EmailParseParams {
        properties: None,
        body_properties: None,
        fetch_text_body_values: None,
        fetch_html_body_values: None,
        fetch_all_body_values: None,
        max_body_value_bytes: None,
        extra,
    };

    let blob_ids = [Id::from("legit-blob-1")];
    sc.email_parse(&blob_ids, Some(params))
        .await
        .expect("email_parse: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be JSON");
    let args = &body["methodCalls"][0][1];

    assert_eq!(
        args["accountId"],
        json!("A13824"),
        "extras MUST NOT override accountId; got {args:?}"
    );
    assert_eq!(
        args["blobIds"],
        json!(["legit-blob-1"]),
        "extras MUST NOT override blobIds; got {args:?}"
    );
}

/// Email/get: legitimate vendor extras (non-reserved keys) MUST still
/// pass through unchanged. This guards against an over-correction that
/// would drop all extras instead of just protecting the reserved keys.
#[tokio::test]
async fn email_get_extras_passthrough_for_vendor_fields() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Email/get",
            { "accountId": "A13824", "state": "s5", "list": [], "notFound": [] },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let mut extra = serde_json::Map::new();
    extra.insert("acmeCorpHint".to_owned(), json!("performance-mode"));
    extra.insert("siteCustomFlag".to_owned(), json!(true));
    let params = EmailGetParams {
        body_properties: None,
        fetch_text_body_values: None,
        fetch_html_body_values: None,
        fetch_all_body_values: None,
        max_body_value_bytes: None,
        extra,
    };

    sc.email_get(None, None, Some(params))
        .await
        .expect("email_get: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be JSON");
    let args = &body["methodCalls"][0][1];

    assert_eq!(
        args["acmeCorpHint"],
        json!("performance-mode"),
        "vendor extras must round-trip onto the wire"
    );
    assert_eq!(
        args["siteCustomFlag"],
        json!(true),
        "vendor extras must round-trip onto the wire"
    );
}
