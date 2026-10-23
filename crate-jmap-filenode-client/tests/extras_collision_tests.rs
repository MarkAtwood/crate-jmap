//! Wire-level tests that a caller-supplied `extra` entry cannot shadow a
//! typed field of the same `*Params` struct (bd:JMAP-tjvm.38).
//!
//! Oracles:
//!   - The `extra` rustdoc contract (workspace AGENTS.md "Caller
//!     contract — wire-key collisions" and the client `*Params` docs):
//!     on collision the typed-field value wins on the wire.
//!   - RFC 8259 §4: "The names within an object SHOULD be unique." The
//!     raw request body must therefore carry each colliding name once.
//!
//! Expected values are the hand-written typed values set by each test,
//! never values produced by the code under test.

#[path = "helpers.rs"]
mod helpers;

use jmap_filenode_client::{FileNodeGetParams, FileNodeSetParams};
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
async fn filenode_get_typed_param_wins_over_colliding_extra() {
    let server = mock(
        "FileNode/get",
        json!({ "accountId": "A13824", "state": "s1", "list": [], "notFound": [] }),
    )
    .await;
    let mut params = FileNodeGetParams {
        fetch_parents: Some(false),
        ..FileNodeGetParams::default()
    };
    params.extra.insert("fetchParents".into(), json!(true));
    params.extra.insert("acmeCorpX".into(), json!("kept"));

    helpers::make_client(&server)
        .filenode_get(None, None, Some(params))
        .await
        .expect("must succeed");

    let (raw, args) = sent_args(&server).await;
    assert_eq!(args["fetchParents"], json!(false));
    assert_eq!(args["acmeCorpX"], json!("kept"));
    assert_eq!(raw.matches("\"fetchParents\"").count(), 1);
}

#[tokio::test]
async fn filenode_set_typed_param_wins_over_colliding_extra() {
    let server = mock(
        "FileNode/set",
        json!({
            "accountId": "A13824",
            "oldState": "s1",
            "newState": "s2",
            "created": null,
            "updated": null,
            "destroyed": ["n1"],
            "notCreated": null,
            "notUpdated": null,
            "notDestroyed": null
        }),
    )
    .await;
    let mut params = FileNodeSetParams {
        on_destroy_remove_children: Some(false),
        ..FileNodeSetParams::default()
    };
    params
        .extra
        .insert("onDestroyRemoveChildren".into(), json!(true));
    params.extra.insert("acmeCorpX".into(), json!("kept"));

    helpers::make_client(&server)
        .filenode_set(None, None, Some(vec![Id::from("n1")]), None, Some(params))
        .await
        .expect("must succeed");

    let (raw, args) = sent_args(&server).await;
    assert_eq!(args["onDestroyRemoveChildren"], json!(false));
    assert_eq!(args["acmeCorpX"], json!("kept"));
    assert_eq!(raw.matches("\"onDestroyRemoveChildren\"").count(), 1);
}
