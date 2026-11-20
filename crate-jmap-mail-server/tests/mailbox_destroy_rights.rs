//! Mailbox/set destroy with onDestroyRemoveEmails removes no Email when
//! the Mailbox itself may not be destroyed.
//!
//! Oracles: RFC 8621 §2 myRights mayDelete "The user may delete the
//! Mailbox itself"; RFC 8620 §5.3 a destroy that "would violate an ACL or
//! other permissions policy" fails with "forbidden"; RFC 8621 §2.5 the
//! Emails are removed as part of destroying the Mailbox;
//! draft-ietf-jmap-mail-sharing-02 §2 `shareWith` gives a sharee "the set
//! of access rights that Principal has for the mailbox".

use std::collections::BTreeMap;

use jmap_mail_server::memory::MemoryBackend;
use jmap_mail_server::{handle_email_get, handle_email_set, handle_mailbox_set};
use jmap_types::Id;
use serde_json::json;

#[tokio::test]
async fn forbidden_destroy_keeps_the_emails() {
    let alice = Id::from("acc");
    let bob = Id::from("bob");
    let b = MemoryBackend::<Id>::default().with_account(&alice);
    // Bob may remove items but not delete the Mailbox.
    let out = handle_mailbox_set(
        &b,
        &alice,
        &BTreeMap::new(),
        json!({"accountId": "acc", "create": {"m": {"name": "M", "shareWith": {"bob": {
            "mayReadItems": true, "mayAddItems": true, "mayRemoveItems": true,
            "maySetSeen": true, "maySetKeywords": true, "mayCreateChild": false,
            "mayRename": false, "mayDelete": false, "maySubmit": false
        }}}}}),
        &["urn:ietf:params:jmap:mail:share".to_owned()],
    )
    .await
    .unwrap();
    let m = out.primary["created"]["m"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let out = handle_email_set(
        &b,
        &alice,
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
        &bob,
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
        &alice,
        json!({"accountId": "acc", "ids": [e.clone()]}),
        &[],
    )
    .await
    .unwrap()
    .primary;
    assert_eq!(got["notFound"], json!([]), "the Email is kept: {got}");
}
