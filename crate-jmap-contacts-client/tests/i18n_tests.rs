//! `SessionClient::with_i18n` on AddressBook/get
//! (draft-atwood-jmap-updates-00 §3, §4.2; decision 32).
//!
//! Oracle: the draft's Appendix A.2 request at jmap-chat-spec 2b85363
//! (`"language": [ "fr-CH", "de" ]`, `urn:ietf:params:jmap:i18n` in
//! `using`), compared with the recorded request body.

#[path = "helpers.rs"]
mod helpers;

use jmap_contacts_client::JmapContactsExt;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn address_book_get_sends_language_and_capability() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "sessionState": "s1",
            "methodResponses": [["AddressBook/get",
                { "accountId": "A1", "state": "s42", "list": [], "notFound": [] }, "r1"]]
        })))
        .mount(&server)
        .await;
    let mut session = helpers::make_session(&server);
    session
        .capabilities
        .insert("urn:ietf:params:jmap:i18n".to_owned(), json!({}));
    let sc = jmap_base_client::JmapClient::new(
        jmap_base_client::DefaultTransport,
        jmap_base_client::NoneAuth,
        &server.uri(),
        jmap_base_client::ClientConfig::default(),
    )
    .unwrap()
    .with_contacts_session(session)
    .with_i18n(Some(vec!["fr-CH".parse().unwrap(), "de".parse().unwrap()]))
    .unwrap();
    sc.address_book_get(None, Some(&["name", "locale"]))
        .await
        .unwrap();
    let reqs = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert!(
        body["using"]
            .as_array()
            .unwrap()
            .contains(&json!("urn:ietf:params:jmap:i18n"))
    );
    assert_eq!(
        body["methodCalls"][0][1]["language"],
        json!(["fr-CH", "de"])
    );
    // The unused helper keeps the shared file warning-free.
    let _ = helpers::make_client;
}
