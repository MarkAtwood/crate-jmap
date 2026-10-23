//! RFC 8620 §5.3 `ifInState` on every contacts-client `/set` method.
//!
//! Oracle: RFC 8620 §5.3, "ifInState: `String|null` This is a state
//! string as returned by the `Foo/get` method ... If supplied, the string
//! must match the current state; otherwise, the method will be aborted
//! and a `stateMismatch` error returned. If null, any changes will be
//! applied to the current state." Each call is checked on the recorded
//! request body only; the mock's empty response is irrelevant here.

#[path = "helpers.rs"]
mod helpers;

use jmap_types::{Id, State};
use serde_json::{Value, json};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn serve() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "sessionState": "s1", "methodResponses": [] })),
        )
        .mount(&server)
        .await;
    server
}

/// `(method name, ifInState argument)` of each request sent.
async fn sent(server: &MockServer) -> Vec<(String, Option<Value>)> {
    let reqs = server.received_requests().await.expect("recorded requests");
    reqs.iter()
        .map(|r| {
            let body: Value = serde_json::from_slice(&r.body).expect("JSON body");
            let call = &body["methodCalls"][0];
            let name = call[0].as_str().expect("method name").to_owned();
            (name, call[1].get("ifInState").cloned())
        })
        .collect()
}

async fn send_all(server: &MockServer, if_in_state: Option<&State>) {
    let sc = helpers::make_client(server);
    let d = || Some(vec![Id::from("x")]);
    let _ = sc.contact_card_set(None, None, d(), if_in_state).await;
    let _ = sc
        .address_book_set(None, None, d(), if_in_state, None)
        .await;
}

const METHODS: [&str; 2] = ["ContactCard/set", "AddressBook/set"];

#[tokio::test]
async fn some_if_in_state_is_sent() {
    let server = serve().await;
    send_all(&server, Some(&State::from("s-guard"))).await;
    let want: Vec<_> = METHODS
        .iter()
        .map(|m| (m.to_string(), Some(json!("s-guard"))))
        .collect();
    assert_eq!(sent(&server).await, want);
}

#[tokio::test]
async fn none_if_in_state_is_omitted() {
    let server = serve().await;
    send_all(&server, None).await;
    let want: Vec<_> = METHODS.iter().map(|m| (m.to_string(), None)).collect();
    assert_eq!(sent(&server).await, want);
}
