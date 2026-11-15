//! `SessionClient::with_i18n` on Calendar/get
//! (draft-atwood-jmap-updates-00 §3, §4.2, §5.5; decision 32).
//!
//! Oracle: the draft's §4.2 argument (`language: String[]|null`) and §3
//! (`urn:ietf:params:jmap:i18n` in `using`), compared with the recorded
//! request body.

#[path = "helpers.rs"]
mod helpers;

use jmap_calendars_client::JmapCalendarsExt;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn calendar_get_sends_language_and_capability() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "sessionState": "s1",
            "methodResponses": [["Calendar/get",
                { "accountId": "A1", "state": "s1", "list": [], "notFound": [] }, "r1"]]
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
    .with_calendars_session(session)
    .with_i18n(Some(vec!["de-AT".parse().unwrap(), "de".parse().unwrap()]))
    .unwrap();
    sc.calendar_get(None, None).await.unwrap();
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
        json!(["de-AT", "de"])
    );
    let _ = helpers::make_client;
}
