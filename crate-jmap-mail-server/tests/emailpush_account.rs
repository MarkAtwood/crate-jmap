//! EmailPushAccount defaults (bd JMAP-r44n.10). Oracle: decision 10 in
//! docs/design/DECISIONS.md: Email history is off unless the deployment
//! says the account's object-history capability lists Email.

use jmap_mail_server::JMAP_EMAILPUSH_URI;
use jmap_mail_server::emailpush::{EmailCondition, EmailPushAccount, EmailPushError};

#[test]
fn new_account_has_only_the_mail_flag() {
    let a = EmailPushAccount::new(false);
    assert!(!a.mail && !a.object_ids && !a.email_history);
    assert!(a.filter_conditions.is_empty() && a.properties.is_empty());
    assert!(EmailPushAccount::new(true).mail);
}

#[test]
fn builders_set_each_field() {
    let a = EmailPushAccount::new(false)
        .with_object_ids(true)
        .with_email_history(true)
        .with_filter_conditions([EmailCondition::Text])
        .with_properties(["preview"]);
    assert!(a.object_ids && a.email_history);
    assert!(a.filter_conditions.contains(&EmailCondition::Text));
    assert!(a.properties.contains("preview"));
}

#[test]
fn uri_and_errors() {
    assert_eq!(JMAP_EMAILPUSH_URI, "urn:ietf:params:jmap:emailpush");
    assert_ne!(
        EmailPushError::LimitTooSmall.to_string(),
        EmailPushError::MissingThreadKeywords.to_string()
    );
}
