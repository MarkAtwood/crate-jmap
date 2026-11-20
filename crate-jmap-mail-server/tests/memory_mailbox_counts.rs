//! The memory backend keeps Mailbox counts, the Mailbox state, Threads and
//! the Message-ID index in step with every Email change.
//!
//! Oracles:
//! - RFC 8621 §2: totalEmails "The number of Emails in this Mailbox";
//!   unreadEmails those "that have neither the "$seen" keyword nor the
//!   "$draft" keyword"; totalThreads "The number of Threads where at least
//!   one Email in the Thread is in this Mailbox"; unreadThreads, here the
//!   RFC's "simplest solution": Threads "where at least one Email in the
//!   Thread is both in this Mailbox and has neither the "$seen" nor
//!   "$draft" keywords".
//! - RFC 8620 §5.1 state: "If the data changes, this string MUST change."
//! - RFC 8621 §2.2 Mailbox/changes `updatedProperties`.
//! - RFC 8621 §3 Thread emailIds: "The ids of the Emails in the Thread".
//! - RFC 8621 §4.8 Email/import alreadyExists names an existing Email.

use std::collections::BTreeMap;

use jmap_mail_server::memory::MemoryBackend;
use jmap_mail_server::{
    handle_email_import, handle_email_set, handle_mailbox_changes, handle_mailbox_get,
    handle_mailbox_set, handle_thread_get,
};
use jmap_types::Id;
use serde_json::{Value, json};

const ACCOUNT: &str = "acc";
const MESSAGE: &[u8] =
    b"From: a@example.com\r\nMessage-ID: <m1@example.com>\r\nSubject: s\r\n\r\nbody\r\n";

async fn setup() -> (MemoryBackend<()>, String, String) {
    let b = MemoryBackend::<()>::default();
    b.register_account(&Id::from(ACCOUNT));
    let out = handle_mailbox_set(
        &b,
        &(),
        &BTreeMap::new(),
        json!({"accountId": ACCOUNT, "create": {"i": {"name": "Inbox"}, "a": {"name": "Archive"}}}),
        &[],
    )
    .await
    .unwrap();
    let id = |k: &str| out.primary["created"][k]["id"].as_str().unwrap().to_owned();
    (b, id("i"), id("a"))
}

async fn email_set(b: &MemoryBackend<()>, args: Value) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT);
    handle_email_set(b, &(), &BTreeMap::new(), args, &[])
        .await
        .unwrap()
        .primary
}

/// Create an Email in `mailbox`, as a reply to `in_reply_to` when given.
async fn create(
    b: &MemoryBackend<()>,
    mailbox: &str,
    message_id: &str,
    in_reply_to: Option<&str>,
) -> String {
    let mut obj = json!({
        "mailboxIds": {mailbox: true},
        "messageId": [message_id],
        "subject": "s",
        "bodyStructure": {"partId": "1", "type": "text/plain"},
        "bodyValues": {"1": {"value": "x"}}
    });
    if let Some(r) = in_reply_to {
        obj["inReplyTo"] = json!([r]);
    }
    let resp = email_set(b, json!({"create": {"c": obj}})).await;
    resp["created"]["c"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{resp}"))
        .to_owned()
}

/// `[totalEmails, unreadEmails, totalThreads, unreadThreads]` of `mailbox`.
async fn counts(b: &MemoryBackend<()>, mailbox: &str) -> [u64; 4] {
    let got = handle_mailbox_get(b, &(), json!({"accountId": ACCOUNT, "ids": [mailbox]}), &[])
        .await
        .unwrap()
        .primary;
    let m = &got["list"][0];
    [
        "totalEmails",
        "unreadEmails",
        "totalThreads",
        "unreadThreads",
    ]
    .map(|k| m[k].as_u64().unwrap_or_else(|| panic!("{k}: {got}")))
}

async fn mailbox_state(b: &MemoryBackend<()>) -> String {
    let got = handle_mailbox_get(b, &(), json!({"accountId": ACCOUNT, "ids": []}), &[])
        .await
        .unwrap()
        .primary;
    got["state"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn counts_follow_create_reply_keywords_move_and_destroy() {
    let (b, inbox, archive) = setup().await;
    let m1 = create(&b, &inbox, "m1@example.com", None).await;
    assert_eq!(counts(&b, &inbox).await, [1, 1, 1, 1]);

    // A reply in the same Thread, filed elsewhere: new to that Mailbox.
    let m2 = create(&b, &archive, "m2@example.com", Some("m1@example.com")).await;
    assert_eq!(counts(&b, &archive).await, [1, 1, 1, 1]);

    email_set(
        &b,
        json!({"update": {m1.clone(): {"keywords/$seen": true}}}),
    )
    .await;
    assert_eq!(counts(&b, &inbox).await, [1, 0, 1, 0]);

    // $draft counts as not unread.
    email_set(
        &b,
        json!({"update": {m2.clone(): {"keywords/$draft": true}}}),
    )
    .await;
    assert_eq!(counts(&b, &archive).await, [1, 0, 1, 0]);

    email_set(
        &b,
        json!({"update": {m1.clone(): {"mailboxIds": {archive.clone(): true}}}}),
    )
    .await;
    assert_eq!(counts(&b, &inbox).await, [0, 0, 0, 0]);
    assert_eq!(counts(&b, &archive).await, [2, 0, 1, 0]);

    email_set(&b, json!({"destroy": [m1, m2]})).await;
    assert_eq!(counts(&b, &archive).await, [0, 0, 0, 0]);
}

#[tokio::test]
async fn count_changes_advance_the_mailbox_state() {
    let (b, inbox, _) = setup().await;
    let before = mailbox_state(&b).await;
    create(&b, &inbox, "m1@example.com", None).await;
    assert_ne!(mailbox_state(&b).await, before);
    let changes = handle_mailbox_changes(
        &b,
        &(),
        json!({"accountId": ACCOUNT, "sinceState": before}),
        &[],
    )
    .await
    .unwrap()
    .primary;
    assert_eq!(changes["updated"], json!([inbox]), "{changes}");
    let mut props: Vec<&str> = changes["updatedProperties"]
        .as_array()
        .unwrap_or_else(|| panic!("{changes}"))
        .iter()
        .filter_map(Value::as_str)
        .collect();
    props.sort_unstable();
    assert_eq!(
        props,
        [
            "totalEmails",
            "totalThreads",
            "unreadEmails",
            "unreadThreads"
        ],
        "{changes}"
    );
}

#[tokio::test]
async fn destroy_updates_threads_and_the_message_id_index() {
    let (b, inbox, _) = setup().await;
    b.store_blob(&Id::from("blob1"), MESSAGE.to_vec());
    let import = |b: &MemoryBackend<()>| {
        let b = b.clone();
        let inbox = inbox.clone();
        async move {
            handle_email_import(
                &b,
                &(),
                &BTreeMap::new(),
                json!({"accountId": ACCOUNT, "emails": {"k": {"blobId": "blob1", "mailboxIds": {inbox: true}}}}),
            )
            .await
            .unwrap()
            .primary
        }
    };
    let first = import(&b).await;
    let e1 = first["created"]["k"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{first}"))
        .to_owned();
    let thread = first["created"]["k"]["threadId"]
        .as_str()
        .unwrap()
        .to_owned();
    let e2 = create(&b, &inbox, "m2@example.com", Some("m1@example.com")).await;

    email_set(&b, json!({"destroy": [e1.clone()]})).await;
    let got = handle_thread_get(
        &b,
        &(),
        json!({"accountId": ACCOUNT, "ids": [thread.clone()]}),
    )
    .await
    .unwrap()
    .primary;
    assert_eq!(got["list"][0]["emailIds"], json!([e2.clone()]), "{got}");

    email_set(&b, json!({"destroy": [e2]})).await;
    let got = handle_thread_get(
        &b,
        &(),
        json!({"accountId": ACCOUNT, "ids": [thread.clone()]}),
    )
    .await
    .unwrap()
    .primary;
    assert_eq!(
        got["notFound"],
        json!([thread]),
        "the emptied Thread is gone: {got}"
    );

    // The destroyed Email no longer owns its Message-ID.
    let again = import(&b).await;
    assert!(again["created"].get("k").is_some(), "{again}");
    assert_ne!(again["created"]["k"]["id"], json!(e1), "{again}");
}
