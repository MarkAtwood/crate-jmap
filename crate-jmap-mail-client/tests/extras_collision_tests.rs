//! Wire-level tests that a caller-supplied `extra` entry cannot shadow a
//! typed field of the same `*Params` / `*Input` struct (bd:JMAP-tjvm.38).
//!
//! Oracles:
//!   - The `extra` rustdoc on every method-argument struct in this crate:
//!     "On collision the typed-field value wins on the wire and the
//!     `extra` value is silently dropped at serialization."
//!   - RFC 8259 §4: "The names within an object SHOULD be unique." The
//!     raw request body must therefore carry each colliding name once.
//!
//! Expected values are the hand-written typed values set by each test,
//! never values produced by the code under test.

#[path = "helpers.rs"]
mod helpers;

use std::collections::BTreeMap;

use jmap_mail_client::{EmailGetParams, EmailImportInput, EmailParseParams};
use jmap_types::Id;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Mounts a single canned method response and returns the server.
async fn mock(method_name: &str, response_args: Value) -> MockServer {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[method_name, response_args, "r1"]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;
    server
}

/// Returns the raw body text and the parsed first method-call arguments.
async fn sent_args(server: &MockServer) -> (String, Value) {
    let reqs = server.received_requests().await.expect("recording enabled");
    let raw = String::from_utf8(reqs[0].body.clone()).expect("body is UTF-8");
    let body: Value = serde_json::from_str(&raw).expect("body is JSON");
    (raw, body["methodCalls"][0][1].clone())
}

#[tokio::test]
async fn email_get_typed_param_wins_over_colliding_extra() {
    let server = mock(
        "Email/get",
        json!({ "accountId": "A13824", "state": "s1", "list": [], "notFound": [] }),
    )
    .await;
    let mut params = EmailGetParams {
        fetch_text_body_values: Some(false),
        max_body_value_bytes: Some(1024),
        ..EmailGetParams::default()
    };
    params
        .extra
        .insert("fetchTextBodyValues".into(), json!(true));
    params.extra.insert("maxBodyValueBytes".into(), json!(1));
    params.extra.insert("acmeCorpX".into(), json!("kept"));

    helpers::make_client(&server)
        .email_get(None, None, Some(params))
        .await
        .expect("must succeed");

    let (raw, args) = sent_args(&server).await;
    assert_eq!(args["fetchTextBodyValues"], json!(false));
    assert_eq!(args["maxBodyValueBytes"], json!(1024));
    assert_eq!(args["acmeCorpX"], json!("kept"));
    assert_eq!(raw.matches("\"fetchTextBodyValues\"").count(), 1);
    assert_eq!(raw.matches("\"maxBodyValueBytes\"").count(), 1);
}

#[tokio::test]
async fn email_parse_typed_param_wins_over_colliding_extra() {
    let server = mock(
        "Email/parse",
        json!({ "accountId": "A13824", "parsed": {}, "notParsable": [], "notFound": [] }),
    )
    .await;
    let mut params = EmailParseParams {
        fetch_all_body_values: Some(false),
        ..EmailParseParams::default()
    };
    params
        .extra
        .insert("fetchAllBodyValues".into(), json!(true));
    params.extra.insert("acmeCorpX".into(), json!("kept"));

    helpers::make_client(&server)
        .email_parse(&[Id::from("blob1")], Some(params))
        .await
        .expect("must succeed");

    let (raw, args) = sent_args(&server).await;
    assert_eq!(args["fetchAllBodyValues"], json!(false));
    assert_eq!(args["acmeCorpX"], json!("kept"));
    assert_eq!(raw.matches("\"fetchAllBodyValues\"").count(), 1);
}

#[tokio::test]
async fn email_import_typed_field_wins_over_colliding_extra() {
    let server = mock(
        "Email/import",
        json!({ "accountId": "A13824", "newState": "s2", "created": null, "notCreated": null }),
    )
    .await;
    let blob = Id::from("REAL");
    let mboxes = [Id::from("mb1")];
    let mut extra = serde_json::Map::new();
    extra.insert("blobId".into(), json!("SPOOF"));
    extra.insert("mailboxIds".into(), json!({ "evil": true }));
    extra.insert("acmeCorpX".into(), json!("kept"));
    let mut emails = BTreeMap::new();
    emails.insert(
        "k1".to_owned(),
        EmailImportInput {
            blob_id: &blob,
            mailbox_ids: &mboxes,
            keywords: None,
            received_at: None,
            extra,
        },
    );

    helpers::make_client(&server)
        .email_import(&emails, None)
        .await
        .expect("must succeed");

    let (raw, args) = sent_args(&server).await;
    let entry = &args["emails"]["k1"];
    assert_eq!(entry["blobId"], json!("REAL"));
    assert_eq!(entry["mailboxIds"], json!({ "mb1": true }));
    assert_eq!(entry["acmeCorpX"], json!("kept"));
    assert_eq!(raw.matches("\"blobId\"").count(), 1);
    assert!(!raw.contains("SPOOF"));
}
