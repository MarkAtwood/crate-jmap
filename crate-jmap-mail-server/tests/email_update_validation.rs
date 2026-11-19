//! Email/set update checks `keywords` and `mailboxIds` as create does.
//!
//! Oracles, RFC 8621 §4.1.1: a keyword "MUST NOT include any of these
//! characters: ( ) { ] % * " \"; "servers MUST return keywords in
//! lowercase"; for keywords and mailboxIds "The value for each key in the
//! object MUST be true"; "An Email in the mail store MUST belong to one or
//! more Mailboxes at all times".

use std::collections::BTreeMap;

use jmap_mail_server::memory::MemoryBackend;
use jmap_mail_server::{handle_email_get, handle_email_set, handle_mailbox_set};
use jmap_types::Id;
use serde_json::{Value, json};

const ACCOUNT: &str = "acc";

/// A backend with Mailboxes m1, m2 and one Email in m1; returns its id.
async fn setup() -> (MemoryBackend, String, String, String) {
    let b = MemoryBackend::new();
    b.register_account(&Id::from(ACCOUNT));
    let out = handle_mailbox_set(
        &b,
        &(),
        &BTreeMap::new(),
        json!({"accountId": ACCOUNT, "create": {"a": {"name": "A"}, "b": {"name": "B"}}}),
        &[],
    )
    .await
    .unwrap();
    let m = |k: &str| out.primary["created"][k]["id"].as_str().unwrap().to_owned();
    let (m1, m2) = (m("a"), m("b"));
    let resp = set(
        &b,
        json!({"create": {"c": {
            "mailboxIds": {m1.clone(): true}, "subject": "s",
            "bodyStructure": {"partId": "1", "type": "text/plain"},
            "bodyValues": {"1": {"value": "x"}}
        }}}),
    )
    .await;
    let e = resp["created"]["c"]["id"].as_str().unwrap().to_owned();
    (b, e, m1, m2)
}

async fn set(b: &MemoryBackend, mut args: Value) -> Value {
    args["accountId"] = json!(ACCOUNT);
    handle_email_set(b, &(), &BTreeMap::new(), args, &[])
        .await
        .unwrap()
        .primary
}

async fn get(b: &MemoryBackend, id: &str) -> Value {
    handle_email_get(
        b,
        &(),
        json!({"accountId": ACCOUNT, "ids": [id], "properties": ["keywords", "mailboxIds"]}),
        &[],
    )
    .await
    .unwrap()
    .primary["list"][0]
        .clone()
}

#[tokio::test]
async fn invalid_patches_are_rejected_and_change_nothing() {
    let (b, e, m1, _) = setup().await;
    let before = get(&b, &e).await;
    for (patch, prop) in [
        (json!({"keywords": {"Bad(kw": true}}), "keywords"),
        (json!({"keywords": {"$flagged": false}}), "keywords"),
        (json!({"keywords/$flagged": false}), "keywords"),
        (json!({"keywords/bad%kw": true}), "keywords"),
        (json!({"mailboxIds": {}}), "mailboxIds"),
        (json!({"mailboxIds": {m1.clone(): false}}), "mailboxIds"),
        (json!({format!("mailboxIds/{m1}"): null}), "mailboxIds"),
    ] {
        let resp = set(&b, json!({"update": {e.clone(): patch.clone()}})).await;
        assert_eq!(
            resp["notUpdated"][&e]["type"], "invalidProperties",
            "{patch}: {resp}"
        );
        assert_eq!(
            resp["notUpdated"][&e]["properties"],
            json!([prop]),
            "{patch}: {resp}"
        );
    }
    assert_eq!(get(&b, &e).await, before);
}

#[tokio::test]
async fn keywords_are_lowercased() {
    let (b, e, ..) = setup().await;
    set(&b, json!({"update": {e.clone(): {"keywords/$Seen": true}}})).await;
    assert_eq!(get(&b, &e).await["keywords"], json!({"$seen": true}));
    set(
        &b,
        json!({"update": {e.clone(): {"keywords": {"$Flagged": true}}}}),
    )
    .await;
    assert_eq!(get(&b, &e).await["keywords"], json!({"$flagged": true}));
}

#[tokio::test]
async fn moving_by_paths_keeps_one_mailbox() {
    let (b, e, m1, m2) = setup().await;
    let resp = set(
        &b,
        json!({"update": {e.clone(): {format!("mailboxIds/{m1}"): null, format!("mailboxIds/{m2}"): true}}}),
    )
    .await;
    assert!(resp["updated"].get(&e).is_some(), "{resp}");
    assert_eq!(get(&b, &e).await["mailboxIds"], json!({m2: true}));
}
