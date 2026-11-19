//! EmailPush client helpers (bd JMAP-r44n.11).
//!
//! Oracles: draft-ietf-jmap-emailpush-05 §4 (the capability in `using`)
//! and §5 Figures 2 and 3; RFC 8620 §7.1 (the StateChange example) and
//! §7.2.3 (the PushVerification example), copied by hand.

#[path = "helpers.rs"]
mod helpers;

use std::collections::BTreeMap;

use jmap_mail_client::{EmailPushSubscriptionInput, PushPayload, parse_push_payload};
use jmap_mail_types::EmailPushConfig;
use jmap_types::{Id, PushKeys, Urgency};
use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const EMAILPUSH: &str = "urn:ietf:params:jmap:emailpush";

#[test]
fn parses_figure_3() {
    let raw = br#"{
     "@type": "EmailPush",
     "accountId": "A1412",
     "emails": [{
       "id": "M699bb151ca4cbf1c24364a68",
       "from": [{ "name": "John Smith", "email": "john@example.com" }],
       "subject": "Surprise birthday party for Jane tonight"
     }],
     "state": null
   }"#;
    let PushPayload::EmailPush(p) = parse_push_payload(raw).unwrap() else {
        panic!("not an EmailPush")
    };
    assert_eq!(p.account_id, Id::from("A1412"));
    assert_eq!(
        p.emails[0]["subject"],
        "Surprise birthday party for Jane tonight"
    );
    assert_eq!(p.state, None);
}

#[test]
fn parses_rfc_8620_examples() {
    let state_change = br#"{
      "@type": "StateChange",
      "changed": {
        "a3123": {
          "Email": "d35ecb040aab",
          "EmailDelivery": "428d565f2440",
          "CalendarEvent": "87accfac587a"
        },
        "a43461d": {
          "Mailbox": "0af7a512ce70",
          "CalendarEvent": "7a4297cecd76"
        }
      }
    }"#;
    let PushPayload::StateChange(s) = parse_push_payload(state_change).unwrap() else {
        panic!("not a StateChange")
    };
    assert_eq!(
        s.changed[&Id::from("a3123")]["EmailDelivery"].as_ref(),
        "428d565f2440"
    );

    let verification = br#"{
      "@type": "PushVerification",
      "pushSubscriptionId": "P43dcfa4-1dd4-41ef-9156-2c89b3b19c60",
      "verificationCode": "da1f097b11ca17f06424e30bf02bfa67"
    }"#;
    let PushPayload::PushVerification(v) = parse_push_payload(verification).unwrap() else {
        panic!("not a PushVerification")
    };
    assert_eq!(v.verification_code, "da1f097b11ca17f06424e30bf02bfa67");
}

#[test]
fn other_and_bad_payloads() {
    assert_eq!(
        parse_push_payload(br#"{"@type": "Vendor", "x": 1}"#).unwrap(),
        PushPayload::Other(json!({"@type": "Vendor", "x": 1}))
    );
    assert!(parse_push_payload(b"[]").is_err());
    assert!(parse_push_payload(b"not json").is_err());
    // A known @type that does not decode.
    assert!(parse_push_payload(br#"{"@type": "EmailPush"}"#).is_err());
}

fn figure_2_config() -> EmailPushConfig {
    serde_json::from_value(json!({
        "filter": {"operator": "OR", "conditions": [
            {"inMailbox": "674cc240-95db-49ce-a8a2-054f4f733095"},
            {"hasKeyword": "$notify"}]},
        "properties": ["id", "from", "subject"]
    }))
    .unwrap()
}

async fn expect(server: &MockServer, request: Value, response: Value) {
    Mock::given(method("POST"))
        .and(path("/api/"))
        .and(body_json(request))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "methodResponses": [response], "sessionState": "s1"
        })))
        .expect(1)
        .mount(server)
        .await;
}

/// Figure 2's request, through the typed input: keys are required, and the
/// emailpush capability is in `using`.
#[tokio::test]
async fn create_sends_figure_2() {
    let server = MockServer::start().await;
    let config = serde_json::to_value(figure_2_config()).unwrap();
    expect(
        &server,
        json!({"using": ["urn:ietf:params:jmap:core", EMAILPUSH],
               "methodCalls": [["PushSubscription/set", {"create": {"c": {
                   "deviceClientId": "a889-ffea-910",
                   "url": "https://push.example.com/WmYD3enMWsSOE0nd",
                   "keys": {"p256dh": "client-p256dh", "auth": "uwPFKACOLBipGPTVz4UCDg"},
                   "types": [],
                   "emailPush": {"A1412": config}
               }}}, "0"]]}),
        json!(["PushSubscription/set", {"created": {"c": {"id": "P1"}}}, "0"]),
    )
    .await;
    let keys: PushKeys = serde_json::from_value(
        json!({"p256dh": "client-p256dh", "auth": "uwPFKACOLBipGPTVz4UCDg"}),
    )
    .unwrap();
    let input = EmailPushSubscriptionInput::new(
        "a889-ffea-910",
        "https://push.example.com/WmYD3enMWsSOE0nd",
        keys,
        BTreeMap::from([(Id::from("A1412"), figure_2_config())]),
    )
    .with_types(Some(vec![]));
    let r = helpers::make_client(&server)
        .email_push_subscription_create(&input)
        .await
        .unwrap();
    assert_eq!(r.created["c"].id, Some(Id::from("P1")));
}

#[tokio::test]
async fn set_and_patch_email_push() {
    let server = MockServer::start().await;
    expect(
        &server,
        json!({"using": ["urn:ietf:params:jmap:core", EMAILPUSH],
               "methodCalls": [["PushSubscription/set",
                   {"update": {"P1": {"emailPush": null}}}, "0"]]}),
        json!(["PushSubscription/set", {"updated": {"P1": null}}, "0"]),
    )
    .await;
    let client = helpers::make_client(&server);
    let r = client.set_email_push(&Id::from("P1"), None).await.unwrap();
    assert_eq!(r.updated[&Id::from("P1")], None);

    let server = MockServer::start().await;
    let high = EmailPushConfig::default().with_urgency(Urgency::High);
    expect(
        &server,
        json!({"using": ["urn:ietf:params:jmap:core", EMAILPUSH],
               "methodCalls": [["PushSubscription/set",
                   {"update": {"P1": {"emailPush/A1412": {
                       "filter": null, "properties": [], "urgency": "high"}}}}, "0"]]}),
        json!(["PushSubscription/set", {"updated": {"P1": null}}, "0"]),
    )
    .await;
    let client = helpers::make_client(&server);
    client
        .patch_email_push(&Id::from("P1"), &Id::from("A1412"), Some(&high))
        .await
        .unwrap();
}
