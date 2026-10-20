//! Wire-shape tests for `email_copy` and the RFC 8620 §5.4 standard /copy
//! arguments.
//!
//! Oracle: RFC 8620 §5.4 names the optional /copy arguments
//! `ifFromInState: "String|null"`, `ifInState: "String|null"`,
//! `onSuccessDestroyOriginal: "Boolean" (default: false)` and
//! `destroyFromIfInState: "String|null"`. The expected request bodies
//! below are hand-written from that text.

#[path = "helpers.rs"]
mod helpers;

use jmap_mail_client::EmailCopyParams;
use jmap_types::Id;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn copy_server() -> MockServer {
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
    server
}

async fn sent_args(server: &MockServer) -> serde_json::Value {
    let reqs = server.received_requests().await.expect("recorded requests");
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).expect("JSON body");
    body["methodCalls"][0][1].clone()
}

/// All four RFC 8620 §5.4 optional arguments reach the wire under their
/// spec names.
#[tokio::test]
async fn email_copy_sends_rfc8620_copy_args() {
    let server = copy_server().await;
    let sc = helpers::make_client(&server);
    let params = EmailCopyParams {
        from_account_id: Id::from("src"),
        if_from_in_state: Some("from-s5".into()),
        if_in_state: Some("to-s7".into()),
        on_success_destroy_original: Some(true),
        destroy_from_if_in_state: Some("from-s5".into()),
        extra: serde_json::Map::new(),
    };
    sc.email_copy(params, json!({ "k1": { "id": "m1" } }))
        .await
        .expect("email_copy must succeed");

    assert_eq!(
        sent_args(&server).await,
        json!({
            "accountId": "A13824",
            "fromAccountId": "src",
            "ifFromInState": "from-s5",
            "ifInState": "to-s7",
            "onSuccessDestroyOriginal": true,
            "destroyFromIfInState": "from-s5",
            "create": { "k1": { "id": "m1" } }
        })
    );
}

/// Unset optional arguments are omitted, leaving the server defaults of
/// RFC 8620 §5.4 in force.
#[tokio::test]
async fn email_copy_omits_unset_copy_args() {
    let server = copy_server().await;
    let sc = helpers::make_client(&server);
    let params = EmailCopyParams {
        from_account_id: Id::from("src"),
        if_from_in_state: None,
        if_in_state: None,
        on_success_destroy_original: None,
        destroy_from_if_in_state: None,
        extra: serde_json::Map::new(),
    };
    sc.email_copy(params, json!({ "k1": { "id": "m1" } }))
        .await
        .expect("email_copy must succeed");

    assert_eq!(
        sent_args(&server).await,
        json!({
            "accountId": "A13824",
            "fromAccountId": "src",
            "create": { "k1": { "id": "m1" } }
        })
    );
}
