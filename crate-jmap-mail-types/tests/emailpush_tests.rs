//! draft-ietf-jmap-emailpush-05 wire types (bd JMAP-r44n.7).
//!
//! Oracles: the §5 figures, copied by hand from the draft, and the §3/§4
//! property definitions; the normalized config form is decision 9 in
//! docs/design/DECISIONS.md.
#![cfg(feature = "emailpush")]

use std::collections::BTreeMap;

use jmap_mail_types::{
    EMAIL_PUSH_PROPERTY, EmailPush, EmailPushAccountCapability, EmailPushCapability,
    EmailPushConfig, JMAP_EMAILPUSH_URI, email_push, set_email_push,
};
use jmap_types::{Id, PushSubscription, State, Urgency};
use serde_json::{Value, json};

/// §5 Figure 1: the Session `capabilities` value, with the emailpush URI
/// mapping to `{}`.
#[test]
fn session_capabilities_figure() {
    let caps = json!({
      "urn:ietf:params:jmap:core": {
        "maxCallsInRequest": 50,
        "maxConcurrentUpload": 10,
        "maxObjectsInGet": 4096,
        "maxObjectsInSet": 4096,
        "collationAlgorithms": ["i;ascii-numeric", "i;ascii-casemap", "i;octet"],
        "maxSizeUpload": 250000000,
        "maxSizeRequest": 10000000,
        "maxConcurrentRequests": 10
      },
      "urn:ietf:params:jmap:emailpush": {}
    });
    assert_eq!(JMAP_EMAILPUSH_URI, "urn:ietf:params:jmap:emailpush");
    let cap: EmailPushCapability =
        serde_json::from_value(caps[JMAP_EMAILPUSH_URI].clone()).unwrap();
    assert_eq!(serde_json::to_value(&cap).unwrap(), json!({}));
    let acct: EmailPushAccountCapability = serde_json::from_value(json!({})).unwrap();
    assert_eq!(serde_json::to_value(&acct).unwrap(), json!({}));
}

/// §5 Figure 3, with `"state": null` (the account has no mail capability).
fn figure_3() -> Value {
    json!({
      "@type": "EmailPush",
      "accountId": "A1412",
      "emails": [{
        "id": "M699bb151ca4cbf1c24364a68",
        "from": [{ "name": "John Smith", "email": "john@example.com" }],
        "subject": "Surprise birthday party for Jane tonight"
      }],
      "state": null
    })
}

#[test]
fn email_push_figure_round_trips() {
    let push: EmailPush = serde_json::from_value(figure_3()).unwrap();
    assert_eq!(push.account_id, Id::from("A1412"));
    assert_eq!(push.state, None);
    assert_eq!(push.emails.len(), 1);
    assert!(push.extra.is_empty());
    assert_eq!(serde_json::to_value(&push).unwrap(), figure_3());
}

/// §3: "state" is a String when the account supports mail.
#[test]
fn email_push_state_string() {
    let push = EmailPush::new(
        Id::from("A"),
        vec![serde_json::Map::new()],
        Some(State::from("s9")),
    );
    assert_eq!(
        serde_json::to_value(&push).unwrap(),
        json!({"@type": "EmailPush", "accountId": "A", "emails": [{}], "state": "s9"})
    );
}

/// §3 "This MUST be the string "EmailPush"": another value is rejected,
/// and the error does not echo it.
#[test]
fn email_push_wrong_type_rejected() {
    for bad in [
        json!("StateChange"),
        json!("CANARY-r44n"),
        json!({"x": "CANARY-r44n"}),
    ] {
        let mut v = figure_3();
        v["@type"] = bad;
        let err = serde_json::from_value::<EmailPush>(v)
            .unwrap_err()
            .to_string();
        assert!(err.contains("@type must be \"EmailPush\""), "{err}");
        assert!(!err.contains("CANARY"), "{err}");
    }
}

/// A receiver that already dispatched on `@type` may pass the object
/// without it.
#[test]
fn email_push_missing_type_accepted() {
    let mut v = figure_3();
    v.as_object_mut().unwrap().remove("@type");
    let push: EmailPush = serde_json::from_value(v).unwrap();
    assert_eq!(serde_json::to_value(&push).unwrap(), figure_3());
}

#[test]
fn email_push_requires_account_and_emails() {
    for key in ["accountId", "emails"] {
        let mut v = figure_3();
        v.as_object_mut().unwrap().remove(key);
        let err = serde_json::from_value::<EmailPush>(v)
            .unwrap_err()
            .to_string();
        assert!(err.contains(key), "{key}: {err}");
    }
}

#[test]
fn vendor_extras_preserved() {
    let mut v = figure_3();
    v["example.com:x"] = json!([1]);
    let push: EmailPush = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(push.extra["example.com:x"], json!([1]));
    assert_eq!(serde_json::to_value(&push).unwrap(), v);

    let cfg_json =
        json!({"filter": null, "properties": [], "urgency": "low", "example.com:y": true});
    let cfg: EmailPushConfig = serde_json::from_value(cfg_json.clone()).unwrap();
    assert_eq!(cfg.extra["example.com:y"], json!(true));
    assert_eq!(serde_json::to_value(&cfg).unwrap(), cfg_json);
}

/// §4 defaults (decision 9): `{}` is the normalized config with every
/// property filled.
#[test]
fn empty_config_normalizes() {
    let cfg: EmailPushConfig = serde_json::from_value(json!({})).unwrap();
    assert_eq!(cfg, EmailPushConfig::default());
    assert_eq!(cfg.urgency, Urgency::Normal);
    assert_eq!(
        serde_json::to_string(&cfg).unwrap(),
        r#"{"filter":null,"properties":[],"urgency":"normal"}"#
    );
}

/// §5 Figure 2's EmailPushConfig decodes, keeping property order; its
/// normalized form adds only `"urgency": "normal"`.
#[test]
fn figure_2_config() {
    let sent = json!({
      "filter": {
        "operator": "OR",
        "conditions": [
          { "inMailbox": "674cc240-95db-49ce-a8a2-054f4f733095" },
          { "hasKeyword": "$notify" }
        ]
      },
      "properties": ["id", "from", "subject"]
    });
    let cfg: EmailPushConfig = serde_json::from_value(sent.clone()).unwrap();
    assert_eq!(cfg.properties, ["id", "from", "subject"]);
    assert!(cfg.filter.is_some());
    let mut want = sent;
    want["urgency"] = json!("normal");
    assert_eq!(serde_json::to_value(&cfg).unwrap(), want);
}

/// §4 urgency "MUST be an urgency-option" (RFC 8030 §5.3).
#[test]
fn urgency_must_be_an_urgency_option() {
    assert!(serde_json::from_value::<EmailPushConfig>(json!({"urgency": "urgent"})).is_err());
    for (s, u) in [
        ("very-low", Urgency::VeryLow),
        ("low", Urgency::Low),
        ("normal", Urgency::Normal),
        ("high", Urgency::High),
    ] {
        let cfg: EmailPushConfig = serde_json::from_value(json!({"urgency": s})).unwrap();
        assert_eq!(cfg.urgency, u);
    }
}

/// RFC 8621 §4.4.1 has no `foo` condition; the filter fails to decode.
#[test]
fn unknown_filter_condition_rejected() {
    assert!(serde_json::from_value::<EmailPushConfig>(json!({"filter": {"foo": 1}})).is_err());
}

fn subscription(extra: Value) -> PushSubscription {
    let mut v = json!({"deviceClientId": "a889-ffea-910"});
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    serde_json::from_value(v).unwrap()
}

#[test]
fn accessors() {
    assert_eq!(EMAIL_PUSH_PROPERTY, "emailPush");
    let mut sub = subscription(json!({}));
    assert_eq!(email_push(&sub).unwrap(), None);
    let sub_null = subscription(json!({"emailPush": null}));
    assert_eq!(email_push(&sub_null).unwrap(), None);

    let map = BTreeMap::from([(
        Id::from("A1412"),
        EmailPushConfig::new(None, vec!["from".into()]).with_urgency(Urgency::High),
    )]);
    set_email_push(&mut sub, Some(map.clone()));
    assert_eq!(
        sub.extra["emailPush"],
        json!({"A1412": {"filter": null, "properties": ["from"], "urgency": "high"}})
    );
    assert_eq!(email_push(&sub).unwrap(), Some(map));

    set_email_push(&mut sub, None);
    assert_eq!(sub.extra["emailPush"], Value::Null);

    let bad = subscription(json!({"emailPush": {"A": {"urgency": "urgent"}}}));
    assert!(email_push(&bad).is_err());
}
