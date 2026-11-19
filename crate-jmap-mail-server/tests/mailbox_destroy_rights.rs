//! Mailbox/set destroy with onDestroyRemoveEmails removes no Email when
//! the Mailbox itself may not be destroyed.
//!
//! Oracles: RFC 8621 §2 myRights mayDelete "The user may delete the
//! Mailbox itself"; RFC 8620 §5.3 a destroy that "would violate an ACL or
//! other permissions policy" fails with "forbidden"; RFC 8621 §2.5 the
//! Emails are removed as part of destroying the Mailbox.

mod common;

use std::collections::BTreeMap;

use common::{FaultyBackend, with_principal};
use jmap_mail_server::{handle_email_get, handle_email_set, handle_mailbox_set};
use jmap_types::Id;
use serde_json::json;

#[tokio::test]
async fn forbidden_destroy_keeps_the_emails() {
    with_principal();
    let b = FaultyBackend::new();
    b.inner.register_account(&Id::from("acc"));
    // The memory backend gives a new Mailbox all-false myRights.
    let out = handle_mailbox_set(
        &b,
        &(),
        &BTreeMap::new(),
        json!({"accountId": "acc", "create": {"m": {"name": "M"}}}),
        &[],
    )
    .await
    .unwrap();
    let m = out.primary["created"]["m"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let out = handle_email_set(
        &b,
        &(),
        &BTreeMap::new(),
        json!({"accountId": "acc", "create": {"e": {"mailboxIds": {m.clone(): true}}}}),
        &[],
    )
    .await
    .unwrap();
    let e = out.primary["created"]["e"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let resp = handle_mailbox_set(
        &b,
        &(),
        &BTreeMap::new(),
        json!({"accountId": "acc", "destroy": [m.clone()], "onDestroyRemoveEmails": true}),
        &[],
    )
    .await
    .unwrap()
    .primary;
    assert_eq!(resp["notDestroyed"][&m]["type"], "forbidden", "{resp}");
    let got = handle_email_get(
        &b,
        &(),
        json!({"accountId": "acc", "ids": [e.clone()]}),
        &[],
    )
    .await
    .unwrap()
    .primary;
    assert_eq!(got["notFound"], json!([]), "the Email is kept: {got}");
}
