//! Wire-shape tests for the RFC 8620 §7 push types (bd:JMAP-nbk3.1).
//!
//! Oracles are the example objects printed in RFC 8620 §7.1.1 and §7.2.3
//! (rfc8620.txt lines 3541-3554 and 3821-3892), compared as
//! `serde_json::Value` so key order and whitespace do not matter.

use jmap_types::{PushKeys, PushSubscription, PushVerification, StateChange};
use serde_json::{json, Value};

/// RFC 8620 §7.1.1 example StateChange object (rfc8620.txt:3541-3554).
fn rfc_state_change() -> Value {
    json!({
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
    })
}

// RFC 8620 §7.1 (rfc8620.txt:3497-3499): "@type: "String" ... This MUST be
// the string "StateChange"."
#[test]
fn state_change_rfc_example_round_trips_with_type() {
    let raw = rfc_state_change();
    let sc: StateChange = serde_json::from_value(raw.clone()).expect("RFC example must parse");
    assert_eq!(sc.changed.len(), 2);
    assert_eq!(sc.changed["a3123"]["Email"], "d35ecb040aab");
    assert_eq!(sc.changed["a43461d"]["Mailbox"], "0af7a512ce70");
    assert!(
        !sc.extra.contains_key("@type"),
        "@type is the discriminant, not an extension field"
    );
    assert_eq!(serde_json::to_value(&sc).expect("serialize"), raw);
}

// RFC 8620 §7.1 (rfc8620.txt:3497-3499): "This MUST be the string
// "StateChange"."
#[test]
fn state_change_rejects_other_type() {
    let mut raw = rfc_state_change();
    raw["@type"] = json!("PushVerification");
    assert!(serde_json::from_value::<StateChange>(raw).is_err());
}

// RFC 8620 §7.2.2 (rfc8620.txt:3774-3776): "@type: "String" ... This MUST be
// the string "PushVerification"."  Example object at rfc8620.txt:3888-3892.
#[test]
fn push_verification_rfc_example_round_trips_with_type() {
    let raw = json!({
        "@type": "PushVerification",
        "pushSubscriptionId": "P43dcfa4-1dd4-41ef-9156-2c89b3b19c60",
        "verificationCode": "da1f097b11ca17f06424e30bf02bfa67"
    });
    let pv: PushVerification = serde_json::from_value(raw.clone()).expect("RFC example must parse");
    assert_eq!(
        pv.push_subscription_id,
        "P43dcfa4-1dd4-41ef-9156-2c89b3b19c60"
    );
    assert_eq!(pv.verification_code, "da1f097b11ca17f06424e30bf02bfa67");
    assert!(!pv.extra.contains_key("@type"));
    assert_eq!(serde_json::to_value(&pv).expect("serialize"), raw);
}

// RFC 8620 §7.2.2 (rfc8620.txt:3776): "This MUST be the string
// "PushVerification"."
#[test]
fn push_verification_rejects_other_type() {
    let raw = json!({
        "@type": "StateChange",
        "pushSubscriptionId": "P43dcfa4-1dd4-41ef-9156-2c89b3b19c60",
        "verificationCode": "da1f097b11ca17f06424e30bf02bfa67"
    });
    assert!(serde_json::from_value::<PushVerification>(raw).is_err());
}

// RFC 8620 §7.2.3 (rfc8620.txt:3827-3840): the two PushSubscription objects
// returned by the example PushSubscription/get round-trip unchanged.
#[test]
fn push_subscription_rfc_get_examples_round_trip() {
    let list = [
        json!({
            "id": "e50b2c1d-9553-41a3-b0a7-a7d26b599ee1",
            "deviceClientId": "b37ff8001ca0",
            "verificationCode": "b210ef734fe5f439c1ca386421359f7b",
            "expires": "2018-07-31T00:13:21Z",
            "types": [ "Todo" ]
        }),
        json!({
            "id": "f2d0aab5-e976-4e8b-ad4b-b380a5b987e4",
            "deviceClientId": "X8980fc",
            "verificationCode": "f3d4618a9ae15c8b7f5582533786d531",
            "expires": "2018-07-12T05:55:00Z",
            "types": [ "Mailbox", "Email", "EmailDelivery" ]
        }),
    ];
    for raw in list {
        let sub: PushSubscription =
            serde_json::from_value(raw.clone()).expect("RFC example must parse");
        assert_eq!(serde_json::to_value(&sub).expect("serialize"), raw);
    }

    let first: PushSubscription = serde_json::from_value(json!({
        "id": "e50b2c1d-9553-41a3-b0a7-a7d26b599ee1",
        "deviceClientId": "b37ff8001ca0",
        "verificationCode": "b210ef734fe5f439c1ca386421359f7b",
        "expires": "2018-07-31T00:13:21Z",
        "types": [ "Todo" ]
    }))
    .expect("RFC example must parse");
    assert_eq!(
        first.id.as_ref().map(|id| id.as_ref()),
        Some("e50b2c1d-9553-41a3-b0a7-a7d26b599ee1")
    );
    assert_eq!(first.device_client_id, "b37ff8001ca0");
    assert_eq!(
        first.verification_code.as_deref(),
        Some("b210ef734fe5f439c1ca386421359f7b")
    );
    assert_eq!(
        first.expires.as_ref().map(|e| e.as_ref()),
        Some("2018-07-31T00:13:21Z")
    );
    assert_eq!(first.types, Some(vec!["Todo".to_owned()]));
    assert!(first.url.is_none());
    assert!(first.keys.is_none());
}

// RFC 8620 §7.2.3 (rfc8620.txt:3849-3857): the create object in the example
// PushSubscription/set, with "types": null meaning all types.
#[test]
fn push_subscription_rfc_create_example_parses() {
    let sub: PushSubscription = serde_json::from_value(json!({
        "deviceClientId": "a889-ffea-910",
        "url": "https://example.com/push/?device=X8980fc&client=12c6d086",
        "types": null
    }))
    .expect("RFC example must parse");
    assert!(sub.id.is_none());
    assert_eq!(sub.device_client_id, "a889-ffea-910");
    assert_eq!(
        sub.url.as_deref(),
        Some("https://example.com/push/?device=X8980fc&client=12c6d086")
    );
    assert!(sub.types.is_none());
    assert!(sub.keys.is_none());
    assert!(sub.verification_code.is_none());
    assert!(sub.expires.is_none());
}

// RFC 8620 §7.2 (rfc8620.txt:3623-3638): "keys: "Object|null" ... The object
// MUST have the following properties: p256dh: "String" ... auth: "String""
#[test]
fn push_keys_require_p256dh_and_auth() {
    let keys: PushKeys = serde_json::from_value(json!({
        "p256dh": "BNcRdreALRFXTkOOUHK1EtK2wtaz5Ry4YfYCA_0QTpQtUbVlUls0VJXg7A8u-Ts1XbjhazAkj7I99e8QcYP7DkM",
        "auth": "tBHItJI5svbpez7KI4CCXg"
    }))
    .expect("both properties present must parse");
    assert_eq!(keys.auth, "tBHItJI5svbpez7KI4CCXg");

    assert!(
        serde_json::from_value::<PushKeys>(json!({ "auth": "tBHItJI5svbpez7KI4CCXg" })).is_err()
    );
    assert!(serde_json::from_value::<PushKeys>(json!({
        "p256dh": "BNcRdreALRFXTkOOUHK1EtK2wtaz5Ry4YfYCA_0QTpQtUbVlUls0VJXg7A8u-Ts1XbjhazAkj7I99e8QcYP7DkM"
    }))
    .is_err());

    // A subscription carrying keys missing auth is rejected as a whole.
    assert!(serde_json::from_value::<PushSubscription>(json!({
        "deviceClientId": "a889-ffea-910",
        "url": "https://example.com/push/",
        "keys": { "p256dh": "BNcRdreALRFXTkOOUHK1EtK2wtaz5Ry4YfYCA_0QTpQtUbVlUls0VJXg7A8u-Ts1XbjhazAkj7I99e8QcYP7DkM" }
    }))
    .is_err());
}

// draft-atwood-jmap-chat-push-00 §"PushSubscription Extension" (cefc217):
// "This document extends the `PushSubscription` object defined in {{RFC8620}}
// Section 7.2 with one additional property: `chatPush`
// (Id[ChatPushConfig], optional)". jmap-types carries it in `extra`.
#[test]
fn push_subscription_preserves_chat_push_in_extra() {
    let chat_push = json!({
        "u1": {
            "kinds": ["direct", "group"],
            "chatIds": ["01J3XKZQN4MWVT8PPBEHTJ3HX"],
            "properties": ["messageId", "chatId", "hasMention"],
            "urgency": "normal",
            "mentionUrgency": "high"
        }
    });
    let raw = json!({
        "id": "P43dcfa4-1dd4-41ef-9156-2c89b3b19c60",
        "deviceClientId": "a889-ffea-910",
        "url": "https://example.com/push/?device=X8980fc&client=12c6d086",
        "keys": {
            "p256dh": "BNcRdreALRFXTkOOUHK1EtK2wtaz5Ry4YfYCA_0QTpQtUbVlUls0VJXg7A8u-Ts1XbjhazAkj7I99e8QcYP7DkM",
            "auth": "tBHItJI5svbpez7KI4CCXg"
        },
        "expires": "2018-07-13T02:14:29Z",
        "chatPush": chat_push.clone()
    });
    let sub: PushSubscription = serde_json::from_value(raw.clone()).expect("must parse");
    assert_eq!(sub.extra.get("chatPush"), Some(&chat_push));
    assert_eq!(serde_json::to_value(&sub).expect("serialize"), raw);
}
