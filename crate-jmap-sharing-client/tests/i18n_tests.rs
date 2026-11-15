//! `SessionClient::with_i18n` on Principal/get and Principal/query
//! (draft-atwood-jmap-updates-00 §3, §4.2; decision 32).
//!
//! Oracle: the draft's §4.2 argument (`language: String[]|null`) and §3
//! (`urn:ietf:params:jmap:i18n` in `using`), compared with the recorded
//! request bodies.

#[path = "helpers.rs"]
mod helpers;

use jmap_sharing_client::JmapSharingExt;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn principal_get_and_query_send_language() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "sessionState": "s1",
            "methodResponses": [["Principal/get",
                { "accountId": "A1", "state": "s1", "list": [], "notFound": [] }, "r1"]]
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "sessionState": "s1",
            "methodResponses": [["Principal/query",
                { "accountId": "A1", "queryState": "q|lang=fr", "canCalculateChanges": true,
                  "position": 0, "ids": [] }, "r1"]]
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
    .with_sharing_session(session)
    .with_i18n(Some(vec!["fr".parse().unwrap()]))
    .unwrap();
    sc.principal_get(None, None).await.unwrap();
    sc.principal_query::<Value>(None, None, &jmap_base_client::QueryParams::default())
        .await
        .unwrap();
    let reqs = server.received_requests().await.unwrap();
    for r in &reqs {
        let body: Value = serde_json::from_slice(&r.body).unwrap();
        assert!(
            body["using"]
                .as_array()
                .unwrap()
                .contains(&json!("urn:ietf:params:jmap:i18n"))
        );
        assert_eq!(
            body["methodCalls"][0][1]["language"],
            json!(["fr"]),
            "{body}"
        );
    }
    assert_eq!(reqs.len(), 2);
    let _ = helpers::make_client;
}
