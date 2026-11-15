//! `SessionClient::with_i18n` (draft-atwood-jmap-updates-00, decision 32).
//!
//! Oracle: the draft at jmap-chat-spec 2b85363: §3 ("A client that wants
//! the properties and arguments of this document includes
//! urn:ietf:params:jmap:i18n in the using property"), §4.2 (`language:
//! String[]|null` on /get, /query and /queryChanges) and §4.1 (`locale`,
//! `localizations`). Request bodies are compared with hand-written JSON.

#[path = "helpers.rs"]
mod helpers;

use jmap_mail_client::JmapMailExt;
use jmap_types::i18n::LanguageRange;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const I18N: &str = "urn:ietf:params:jmap:i18n";

fn client(server: &MockServer, with_capability: bool) -> jmap_mail_client::SessionClient {
    let mut session = helpers::make_session(server);
    if with_capability {
        session.capabilities.insert(I18N.to_owned(), json!({}));
    }
    jmap_base_client::JmapClient::new(
        jmap_base_client::DefaultTransport,
        jmap_base_client::NoneAuth,
        &server.uri(),
        jmap_base_client::ClientConfig::default(),
    )
    .unwrap()
    .with_mail_session(session)
}

async fn mount(server: &MockServer, method_name: &str, response: Value) {
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "sessionState": "s1",
            "methodResponses": [[method_name, response, "r1"]]
        })))
        .mount(server)
        .await;
}

async fn last_body(server: &MockServer) -> Value {
    let reqs = server.received_requests().await.unwrap();
    serde_json::from_slice(&reqs.last().unwrap().body).unwrap()
}

fn de() -> Vec<LanguageRange> {
    vec!["de-AT".parse().unwrap(), "de".parse().unwrap()]
}

/// Mailbox/get with a language: the capability is in `using`, `language`
/// is sent, and the typed Mailbox carries `locale`.
#[tokio::test]
async fn mailbox_get_sends_language_and_capability() {
    let server = MockServer::start().await;
    mount(
        &server,
        "Mailbox/get",
        json!({
            "accountId": "A13824", "state": "s1", "notFound": [],
            "list": [{
                "id": "m1", "name": "Posteingang", "parentId": null, "role": "inbox",
                "sortOrder": 0, "totalEmails": 0, "unreadEmails": 0, "totalThreads": 0,
                "unreadThreads": 0, "myRights": {
                    "mayReadItems": true, "mayAddItems": true, "mayRemoveItems": true,
                    "maySetSeen": true, "maySetKeywords": true, "mayCreateChild": true,
                    "mayRename": true, "mayDelete": true, "maySubmit": true
                },
                "isSubscribed": true, "locale": "de", "localizations": null
            }]
        }),
    )
    .await;
    let sc = client(&server, true).with_i18n(Some(de())).unwrap();
    let resp = sc.mailbox_get(None, None).await.unwrap();
    let body = last_body(&server).await;
    assert!(
        body["using"].as_array().unwrap().contains(&json!(I18N)),
        "{body}"
    );
    assert_eq!(
        body["methodCalls"][0][1]["language"],
        json!(["de-AT", "de"])
    );
    assert_eq!(resp.list[0].locale.as_ref().map(|t| t.as_str()), Some("de"));
}

/// Mailbox/query sends `language` too (§4.2.2).
#[tokio::test]
async fn mailbox_query_sends_language() {
    let server = MockServer::start().await;
    mount(
        &server,
        "Mailbox/query",
        json!({
            "accountId": "A13824", "queryState": "q1|lang=de-AT,de",
            "canCalculateChanges": true, "position": 0, "ids": []
        }),
    )
    .await;
    let sc = client(&server, true).with_i18n(Some(de())).unwrap();
    sc.mailbox_query::<Value>(None, None, &jmap_base_client::QueryParams::default())
        .await
        .unwrap();
    let body = last_body(&server).await;
    assert_eq!(
        body["methodCalls"][0][1]["language"],
        json!(["de-AT", "de"])
    );
}

/// With `language` None the capability is still in `using` (so `/set` may
/// write locale and localizations), and no `language` is sent.
#[tokio::test]
async fn without_language_only_the_capability() {
    let server = MockServer::start().await;
    mount(
        &server,
        "Mailbox/get",
        json!({ "accountId": "A13824", "state": "s1", "notFound": [], "list": [] }),
    )
    .await;
    let sc = client(&server, true).with_i18n(None).unwrap();
    sc.mailbox_get(None, None).await.unwrap();
    let body = last_body(&server).await;
    assert!(body["using"].as_array().unwrap().contains(&json!(I18N)));
    assert!(body["methodCalls"][0][1].get("language").is_none());
}

/// Without `with_i18n` nothing changes on the wire.
#[tokio::test]
async fn default_client_is_unchanged() {
    let server = MockServer::start().await;
    mount(
        &server,
        "Mailbox/get",
        json!({ "accountId": "A13824", "state": "s1", "notFound": [], "list": [] }),
    )
    .await;
    helpers::make_client(&server)
        .mailbox_get(None, None)
        .await
        .unwrap();
    let body = last_body(&server).await;
    assert!(!body["using"].as_array().unwrap().contains(&json!(I18N)));
    assert!(body["methodCalls"][0][1].get("language").is_none());
}

/// RFC 8620 §3.6.1: an unadvertised capability in `using` fails the whole
/// request, so `with_i18n` refuses a session without it.
#[tokio::test]
async fn refuses_a_session_without_the_capability() {
    let server = MockServer::start().await;
    let err = client(&server, false).with_i18n(None).unwrap_err();
    assert!(
        matches!(err, jmap_base_client::ClientError::InvalidSession(_)),
        "{err:?}"
    );
}
