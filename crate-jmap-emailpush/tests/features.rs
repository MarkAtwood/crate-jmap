//! The facade's re-exports under each feature set.
//!
//! Oracles: draft-ietf-jmap-emailpush-05 (§2.1 the capability URI, §3 the
//! EmailPush object, §4 the EmailPushConfig defaults) and the crate's
//! documented feature table.

use jmap_emailpush::types::{
    EMAIL_PUSH_PROPERTY, EmailPushConfig, JMAP_EMAILPUSH_URI, email_push, set_email_push,
};

#[test]
fn types_are_always_there() {
    assert_eq!(JMAP_EMAILPUSH_URI, "urn:ietf:params:jmap:emailpush");
    assert_eq!(jmap_emailpush::CAPABILITY, JMAP_EMAILPUSH_URI);
    assert_eq!(EMAIL_PUSH_PROPERTY, "emailPush");
    // §4: properties default to [] and urgency to "normal".
    let config: EmailPushConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(
        serde_json::to_value(&config).unwrap(),
        serde_json::json!({"filter": null, "properties": [], "urgency": "normal"})
    );
    // The accessors are re-exported with the types.
    let _read: fn(&_) -> _ = email_push;
    let _write: fn(&mut _, _) = set_email_push;
}

#[cfg(feature = "client")]
#[test]
fn client_is_reexported() {
    use jmap_emailpush::client::{PushPayload, parse_push_payload};

    let payload = br#"{"@type": "EmailPush", "accountId": "A1",
        "emails": [{"id": "M1"}], "state": "s1"}"#;
    match parse_push_payload(payload).unwrap() {
        PushPayload::EmailPush(push) => {
            assert_eq!(push.emails.len(), 1);
            assert_eq!(push.emails[0]["id"], "M1");
        }
        other => panic!("decoded as {other:?}"),
    }
}

#[cfg(feature = "server")]
#[test]
fn server_is_reexported() {
    use jmap_emailpush::server::{EmailPushAccount, check_email_push_config};

    let account = EmailPushAccount::new(true);
    let config = check_email_push_config(&serde_json::json!({}), &account, None).unwrap();
    assert_eq!(config, EmailPushConfig::default());
}
