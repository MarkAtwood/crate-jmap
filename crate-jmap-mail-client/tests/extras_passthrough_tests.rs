//! Wire-level passthrough tests for the workspace extras-preservation
//! policy (bd:JMAP-tjvm.7, JMAP-tjvm.9): every public method-argument
//! struct in this crate that appears on the JMAP wire MUST round-trip
//! its `extra` map onto the wire when the production method is invoked.
//!
//! The struct-level extras tests in `src/methods/mod.rs` verify that
//! each `*Params` struct *serializes* extras correctly. They do NOT
//! verify that the production method actually includes those extras
//! in the JMAP request body sent to the server. Three methods
//! (`email_copy`, `mailbox_set`, `email_submission_set`) previously
//! shipped with the silent-drop bug — extras vanished between the
//! caller and the wire. These tests guard against regression.
//!
//! Oracles:
//!   - Workspace AGENTS.md "Extras-preservation policy for vendor/site
//!     fields": method-argument structs in `*-client` crates are in
//!     scope; the `extra` map must round-trip across the wire.
//!   - RFC 8620 §1.6 — unknown fields are silently ignored by the
//!     server, but the client MUST emit them so the server gets a
//!     chance to process them.

#[path = "helpers.rs"]
mod helpers;

use std::collections::HashMap;

use jmap_mail_client::{
    EmailCopyParams, EmailGetParams, EmailImportInput, EmailParseParams, EmailSubmissionSetParams,
    MailboxSetParams,
};
use jmap_types::{Id, PatchObject};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// `email_get` must include `params.extra` on the wire.
#[tokio::test]
async fn email_get_routes_vendor_extras_to_wire() {
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
    let mut params = EmailGetParams::default();
    params
        .extra
        .insert("acmeCorpHint".into(), json!("aggressive"));
    sc.email_get(None, None, Some(params))
        .await
        .expect("must succeed");

    let reqs = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
    let args = &body["methodCalls"][0][1];
    assert_eq!(args["acmeCorpHint"], json!("aggressive"));
}

/// `email_copy` must include `params.extra` on the wire.
#[tokio::test]
async fn email_copy_routes_vendor_extras_to_wire() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Email/copy",
            {
                "accountId": "A13824",
                "fromAccountId": "src",
                "oldState": "s1",
                "newState": "s2",
                "created": null,
                "notCreated": null
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
    let mut extra = serde_json::Map::new();
    extra.insert("acmeCorpAudit".into(), json!(true));
    let params = EmailCopyParams {
        from_account_id: Id::from("src"),
        on_success_destroy_original: None,
        destroy_from_if_in_state: None,
        extra,
    };
    sc.email_copy(params, json!({ "k1": { "mailboxIds": { "mb1": true } } }))
        .await
        .expect("must succeed");

    let reqs = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["acmeCorpAudit"],
        json!(true),
        "extras MUST reach the wire (bd:JMAP-tjvm.7 silent-drop regression)"
    );
}

/// `mailbox_set` must include `params.extra` on the wire.
#[tokio::test]
async fn mailbox_set_routes_vendor_extras_to_wire() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Mailbox/set",
            {
                "accountId": "A13824",
                "oldState": "s1",
                "newState": "s2",
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
    let mut params = MailboxSetParams::default();
    params
        .extra
        .insert("acmeCorpCascade".into(), json!("strict"));
    sc.mailbox_set(None, None, None, Some(params))
        .await
        .expect("must succeed");

    let reqs = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["acmeCorpCascade"],
        json!("strict"),
        "extras MUST reach the wire (bd:JMAP-tjvm.7 silent-drop regression)"
    );
}

/// `email_submission_set` must include `params.extra` on the wire.
#[tokio::test]
async fn email_submission_set_routes_vendor_extras_to_wire() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "EmailSubmission/set",
            {
                "accountId": "A13824",
                "oldState": "s1",
                "newState": "s2",
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
    let mut params = EmailSubmissionSetParams::default();
    params
        .extra
        .insert("acmeCorpQueue".into(), json!("priority"));
    let update: HashMap<Id, PatchObject> = HashMap::new();
    sc.email_submission_set(None, Some(update), None, None, Some(params))
        .await
        .expect("must succeed");

    let reqs = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["acmeCorpQueue"],
        json!("priority"),
        "extras MUST reach the wire (bd:JMAP-tjvm.7 silent-drop regression)"
    );
}

/// `email_parse` must include `params.extra` on the wire.
/// Already routed via the iterate-merge in email.rs:345-356 (post-tjvm.1/.2
/// fix). This test guards against any future reshape that drops extras.
#[tokio::test]
async fn email_parse_routes_vendor_extras_to_wire() {
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
    let mut params = EmailParseParams::default();
    params.extra.insert("acmeCorpStrict".into(), json!(true));
    let blobs = [Id::from("blob1")];
    sc.email_parse(&blobs, Some(params))
        .await
        .expect("must succeed");

    let reqs = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
    let args = &body["methodCalls"][0][1];
    assert_eq!(args["acmeCorpStrict"], json!(true));
}

/// `email_import` per-entry `EmailImportInput.extra` round-trip is currently
/// vulnerable to the flatten-shadowing bug documented in bd:JMAP-tjvm.15 +
/// bd:JMAP-tjvm.38. This test asserts the *current* behaviour: vendor
/// extras DO reach the wire (via `to_value` of the input struct). The
/// shadowing-of-typed-fields bug is tracked separately and is not in
/// scope here.
#[tokio::test]
async fn email_import_routes_vendor_extras_to_wire() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Email/import",
            {
                "accountId": "A13824",
                "newState": "s2",
                "created": null,
                "notCreated": null
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
    let blob = Id::from("blob1");
    let mboxes = [Id::from("mb1")];
    let mut extra = serde_json::Map::new();
    extra.insert("acmeCorpSource".into(), json!("mta-relay"));
    let input = EmailImportInput {
        blob_id: &blob,
        mailbox_ids: &mboxes,
        keywords: None,
        received_at: None,
        extra,
    };
    let mut emails = HashMap::new();
    emails.insert("k1".to_owned(), input);
    sc.email_import(&emails, None).await.expect("must succeed");

    let reqs = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
    let args = &body["methodCalls"][0][1];
    assert_eq!(args["emails"]["k1"]["acmeCorpSource"], json!("mta-relay"));
}
