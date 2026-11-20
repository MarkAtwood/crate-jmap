//! `SessionClient::with_mail_sharing` (draft-ietf-jmap-mail-sharing-02).
//!
//! Oracles: the draft §1.2.1 ("When a server advertises this capability,
//! it signifies that the Mailbox data type has been extended with the
//! sharing properties"), §2 (`shareWith: Id[MailboxRights]|null`,
//! `mayShare: Boolean`), and RFC 8620 §3.6.1 (`unknownCapability` for a
//! `using` entry the server does not support). Request bodies are compared
//! with hand-written JSON.

#[path = "helpers.rs"]
mod helpers;

use std::collections::BTreeMap;

use jmap_mail_client::JmapMailExt;
use jmap_types::{Id, PatchObject};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SHARE: &str = "urn:ietf:params:jmap:mail:share";

fn client(server: &MockServer, with_capability: bool) -> jmap_mail_client::SessionClient {
    let mut session = helpers::make_session(server);
    if with_capability {
        session.capabilities.insert(SHARE.to_owned(), json!({}));
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

/// Mailbox/get names the capability, and the typed Mailbox carries
/// `shareWith` and `mayShare`.
#[tokio::test]
async fn mailbox_get_names_the_capability_and_types_share_with() {
    let server = MockServer::start().await;
    let rights = json!({
        "mayReadItems": true, "mayAddItems": false, "mayRemoveItems": false,
        "maySetSeen": true, "maySetKeywords": false, "mayCreateChild": false,
        "mayRename": false, "mayDelete": false, "maySubmit": false, "mayShare": false
    });
    mount(
        &server,
        "Mailbox/get",
        json!({
            "accountId": "A13824", "state": "s1", "notFound": [],
            "list": [{
                "id": "m1", "name": "Team", "parentId": null, "role": null,
                "sortOrder": 0, "totalEmails": 0, "unreadEmails": 0, "totalThreads": 0,
                "unreadThreads": 0, "myRights": {
                    "mayReadItems": true, "mayAddItems": true, "mayRemoveItems": true,
                    "maySetSeen": true, "maySetKeywords": true, "mayCreateChild": true,
                    "mayRename": true, "mayDelete": true, "maySubmit": true, "mayShare": true
                },
                "isSubscribed": true, "shareWith": {"P2": rights}
            }]
        }),
    )
    .await;
    let sc = client(&server, true).with_mail_sharing().unwrap();
    let resp = sc.mailbox_get(None, None).await.unwrap();
    let body = last_body(&server).await;
    assert!(
        body["using"].as_array().unwrap().contains(&json!(SHARE)),
        "{body}"
    );
    let mailbox = &resp.list[0];
    assert_eq!(mailbox.my_rights.may_share, Some(true));
    let p2 = &mailbox.share_with.as_ref().unwrap()[&Id::from("P2")];
    assert!(p2.may_read_items && p2.may_set_seen);
    assert_eq!(p2.may_share, Some(false));
}

/// Mailbox/set sends a `shareWith` patch with the capability in `using`.
#[tokio::test]
async fn mailbox_set_sends_a_share_with_patch() {
    let server = MockServer::start().await;
    mount(
        &server,
        "Mailbox/set",
        json!({
            "accountId": "A13824", "oldState": "s1", "newState": "s2",
            "updated": {"m1": null}
        }),
    )
    .await;
    let sc = client(&server, true).with_mail_sharing().unwrap();
    let patch: PatchObject = serde_json::from_value(json!({"shareWith/P2": null})).unwrap();
    let update = BTreeMap::from([(Id::from("m1"), patch)]);
    sc.mailbox_set(None, Some(update), None, None, None, None)
        .await
        .unwrap();
    let body = last_body(&server).await;
    assert!(
        body["using"].as_array().unwrap().contains(&json!(SHARE)),
        "{body}"
    );
    assert_eq!(
        body["methodCalls"][0][1]["update"],
        json!({"m1": {"shareWith/P2": null}})
    );
}

/// Without `with_mail_sharing` nothing changes on the wire.
#[tokio::test]
async fn without_it_using_is_unchanged() {
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
    assert!(
        !body["using"].as_array().unwrap().contains(&json!(SHARE)),
        "{body}"
    );
}

/// A session without the capability would fail every request with
/// `unknownCapability`, so `with_mail_sharing` refuses it.
#[tokio::test]
async fn refuses_a_session_without_the_capability() {
    let server = MockServer::start().await;
    let err = client(&server, false).with_mail_sharing().unwrap_err();
    assert!(
        matches!(err, jmap_base_client::ClientError::InvalidSession(_)),
        "{err:?}"
    );
}
