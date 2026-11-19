//! `JmapClient::push_subscription_get` / `push_subscription_set`
//! (bd JMAP-r44n.9).
//!
//! Oracles: RFC 8620 §7.2.3's example requests and responses, copied by
//! hand (the create omits `"types": null`, which this crate serializes by
//! leaving the property out); draft-ietf-jmap-emailpush-05 §4 (the
//! capability in `using`) and §5 Figure 2's `emailPush`.

use std::collections::BTreeMap;

use jmap_base_client::{ClientConfig, DefaultTransport, JmapClient, NoneAuth, Session};
use jmap_types::{Id, PatchObject, PushSubscription};
use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const EMAILPUSH: &str = "urn:ietf:params:jmap:emailpush";

async fn setup(request: Value, response: Value) -> (MockServer, JmapClient, Session) {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/"))
        .and(body_json(request))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "methodResponses": [response], "sessionState": "s1"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let client = JmapClient::new(
        DefaultTransport,
        NoneAuth,
        &server.uri(),
        ClientConfig::default(),
    )
    .expect("client");
    let session: Session = serde_json::from_value(json!({
        "capabilities": {}, "accounts": {}, "primaryAccounts": {}, "username": "",
        "apiUrl": format!("{}/api/", server.uri()),
        "downloadUrl": format!("{}/d", server.uri()), "uploadUrl": format!("{}/u", server.uri()),
        "eventSourceUrl": format!("{}/e", server.uri()), "state": ""
    }))
    .expect("session");
    (server, client, session)
}

#[tokio::test]
async fn get_all() {
    let (_s, client, session) = setup(
        json!({"using": ["urn:ietf:params:jmap:core"],
               "methodCalls": [["PushSubscription/get", {"ids": null}, "0"]]}),
        json!(["PushSubscription/get", {
            "list": [{
                "id": "e50b2c1d-9553-41a3-b0a7-a7d26b599ee1",
                "deviceClientId": "b37ff8001ca0",
                "verificationCode": "b210ef734fe5f439c1ca386421359f7b",
                "expires": "2018-07-31T00:13:21Z",
                "types": [ "Todo" ]
            }, {
                "id": "f2d0aab5-e976-4e8b-ad4b-b380a5b987e4",
                "deviceClientId": "X8980fc",
                "verificationCode": "f3d4618a9ae15c8b7f5582533786d531",
                "expires": "2018-07-12T05:55:00Z",
                "types": [ "Mailbox", "Email", "EmailDelivery" ]
            }],
            "notFound": []
        }, "0"]),
    )
    .await;
    let r = client
        .push_subscription_get(&session, None, None, &[])
        .await
        .unwrap();
    assert_eq!(r.list.len(), 2);
    assert_eq!(
        r.list[1].id.as_ref().map(AsRef::as_ref),
        Some("f2d0aab5-e976-4e8b-ad4b-b380a5b987e4")
    );
    assert_eq!(r.list[1].extra["deviceClientId"], "X8980fc");
    assert!(r.not_found.is_empty());
}

/// The capability lands in `using`; `properties` without deviceClientId
/// still decodes.
#[tokio::test]
async fn get_with_extension_property() {
    let (_s, client, session) = setup(
        json!({"using": ["urn:ietf:params:jmap:core", EMAILPUSH],
               "methodCalls": [["PushSubscription/get",
                                {"ids": ["P1"], "properties": ["emailPush"]}, "0"]]}),
        json!(["PushSubscription/get", {
            "list": [{"id": "P1", "emailPush": {"A1412": {"filter": null, "properties": [], "urgency": "normal"}}}],
            "notFound": null
        }, "0"]),
    )
    .await;
    let r = client
        .push_subscription_get(
            &session,
            Some(&[Id::from("P1")]),
            Some(&["emailPush"]),
            &[EMAILPUSH],
        )
        .await
        .unwrap();
    assert_eq!(r.list[0].extra["emailPush"]["A1412"]["urgency"], "normal");
    assert!(r.not_found.is_empty());
}

#[tokio::test]
async fn create_then_verify() {
    let (_s, client, session) = setup(
        json!({"using": ["urn:ietf:params:jmap:core"],
               "methodCalls": [["PushSubscription/set", {"create": {"4f29": {
                   "deviceClientId": "a889-ffea-910",
                   "url": "https://example.com/push/?device=X8980fc&client=12c6d086"
               }}}, "0"]]}),
        json!(["PushSubscription/set", {
            "created": {
                "4f29": {
                    "id": "P43dcfa4-1dd4-41ef-9156-2c89b3b19c60",
                    "keys": null,
                    "expires": "2018-07-13T02:14:29Z"
                }
            }
        }, "0"]),
    )
    .await;
    let sub: PushSubscription = serde_json::from_value(json!({
        "deviceClientId": "a889-ffea-910",
        "url": "https://example.com/push/?device=X8980fc&client=12c6d086"
    }))
    .unwrap();
    let r = client
        .push_subscription_set(
            &session,
            &BTreeMap::from([("4f29".to_owned(), sub)]),
            &BTreeMap::new(),
            &[],
            &[],
        )
        .await
        .unwrap();
    let created = &r.created["4f29"];
    assert_eq!(
        created.id.as_ref().map(AsRef::as_ref),
        Some("P43dcfa4-1dd4-41ef-9156-2c89b3b19c60")
    );
    assert_eq!(created.extra["expires"], "2018-07-13T02:14:29Z");
    assert!(r.updated.is_empty() && r.not_created.is_empty());
}

/// The lifetime extension: an `updated` echo of the value the server
/// changed.
#[tokio::test]
async fn update_echo() {
    let id = "P43dcfa4-1dd4-41ef-9156-2c89b3b19c60";
    let (_s, client, session) = setup(
        json!({"using": ["urn:ietf:params:jmap:core"],
               "methodCalls": [["PushSubscription/set", {"update": {id: {
                   "expires": "2018-08-13T00:00:00Z"
               }}}, "0"]]}),
        json!(["PushSubscription/set", {
            "updated": {id: {"expires": "2018-07-15T02:22:50Z"}}
        }, "0"]),
    )
    .await;
    let patch: PatchObject =
        serde_json::from_value(json!({"expires": "2018-08-13T00:00:00Z"})).unwrap();
    let r = client
        .push_subscription_set(
            &session,
            &BTreeMap::new(),
            &BTreeMap::from([(Id::from(id), patch)]),
            &[],
            &[],
        )
        .await
        .unwrap();
    let echo = r.updated[&Id::from(id)].as_ref().unwrap();
    assert_eq!(echo.extra["expires"], "2018-07-15T02:22:50Z");
}

/// An extension create with its capability in `using`; the normalized
/// value the server stored comes back in `created`, and a failure lands in
/// `notCreated`.
#[tokio::test]
async fn create_with_email_push() {
    let email_push = json!({"A1412": {
        "filter": {"operator": "OR", "conditions": [
            {"inMailbox": "674cc240-95db-49ce-a8a2-054f4f733095"},
            {"hasKeyword": "$notify"}]},
        "properties": ["id", "from", "subject"]
    }});
    let mut normalized = email_push.clone();
    normalized["A1412"]["urgency"] = json!("normal");
    let sub = json!({
        "deviceClientId": "a889-ffea-910",
        "url": "https://push.example.com/WmYD3enMWsSOE0nd",
        "keys": {"p256dh": "client-p256dh", "auth": "client-auth"},
        "types": [],
        "emailPush": email_push
    });
    let (_s, client, session) = setup(
        json!({"using": ["urn:ietf:params:jmap:core", EMAILPUSH],
               "methodCalls": [["PushSubscription/set",
                                {"create": {"4f29": sub.clone(), "bad": sub.clone()}}, "0"]]}),
        json!(["PushSubscription/set", {
            "created": {"4f29": {"id": "P1", "emailPush": normalized.clone()}},
            "notCreated": {"bad": {"type": "invalidProperties",
                                   "properties": ["emailPush", "emailPush/A1412"]}}
        }, "0"]),
    )
    .await;
    let sub: PushSubscription = serde_json::from_value(sub).unwrap();
    let r = client
        .push_subscription_set(
            &session,
            &BTreeMap::from([("4f29".to_owned(), sub.clone()), ("bad".to_owned(), sub)]),
            &BTreeMap::new(),
            &[],
            &[EMAILPUSH, EMAILPUSH],
        )
        .await
        .unwrap();
    assert_eq!(r.created["4f29"].extra["emailPush"], normalized);
    assert_eq!(
        r.not_created["bad"].properties.as_deref(),
        Some(&["emailPush".to_owned(), "emailPush/A1412".to_owned()][..])
    );
}

#[tokio::test]
async fn destroy() {
    let (_s, client, session) = setup(
        json!({"using": ["urn:ietf:params:jmap:core"],
               "methodCalls": [["PushSubscription/set", {"destroy": ["P1"]}, "0"]]}),
        json!(["PushSubscription/set", {"destroyed": ["P1"], "notDestroyed": null}, "0"]),
    )
    .await;
    let r = client
        .push_subscription_set(
            &session,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &[Id::from("P1")],
            &[],
        )
        .await
        .unwrap();
    assert_eq!(r.destroyed, [Id::from("P1")]);
}
