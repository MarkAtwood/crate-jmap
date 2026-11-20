//! Memory `Email/query` evaluates every RFC 8621 §4.4.1 condition through
//! the shared filter evaluator (bd JMAP-r44n.4). `from`, `subject` and
//! `maxSize` used to match every Email.
//!
//! Oracles: RFC 8621 §4.4.1 ("Looks for the text in the From header
//! field", "the size ... must be less than this number"), with the expected
//! ids worked out by hand from the Emails created below.

use std::collections::BTreeMap;

use jmap_mail_server::memory::MemoryBackend;
use jmap_mail_server::{handle_email_get, handle_email_query, handle_email_set};
use jmap_types::Id;
use serde_json::{Value, json};

const ACCOUNT: &str = "account1";

async fn create(b: &MemoryBackend<()>, from: &str, subject: &str, body: &str) -> String {
    let out = handle_email_set(
        b,
        &(),
        &BTreeMap::new(),
        json!({"accountId": ACCOUNT, "create": {"c": {
            "mailboxIds": {"inbox": true},
            "from": [{"name": from, "email": format!("{}@example.com", from.to_lowercase())}],
            "subject": subject,
            "bodyStructure": {"partId": "1", "type": "text/plain"},
            "bodyValues": {"1": {"value": body}}
        }}}),
        &[],
    )
    .await
    .unwrap();
    out.primary["created"]["c"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{}", out.primary))
        .to_owned()
}

async fn query(b: &MemoryBackend<()>, filter: Value) -> Vec<String> {
    let out = handle_email_query(b, &(), json!({"accountId": ACCOUNT, "filter": filter}), &[])
        .await
        .unwrap();
    let mut ids: Vec<String> = out.primary["ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    ids.sort();
    ids
}

fn sorted(mut ids: Vec<String>) -> Vec<String> {
    ids.sort();
    ids
}

async fn setup() -> (MemoryBackend<()>, String, String) {
    let b = MemoryBackend::<()>::default();
    b.register_account(&Id::from(ACCOUNT));
    let alice = create(&b, "Alice", "Quarterly report", "short").await;
    let bob = create(&b, "Bob", "Lunch?", &"long body ".repeat(200)).await;
    (b, alice, bob)
}

#[tokio::test]
async fn from_condition() {
    let (b, alice, bob) = setup().await;
    assert_eq!(
        query(&b, json!({"from": "alice"})).await,
        std::slice::from_ref(&alice)
    );
    assert_eq!(query(&b, json!({"from": "BOB@example"})).await, [bob]);
    assert!(query(&b, json!({"from": "carol"})).await.is_empty());
}

#[tokio::test]
async fn subject_condition() {
    let (b, alice, bob) = setup().await;
    assert_eq!(query(&b, json!({"subject": "REPORT"})).await, [alice]);
    assert_eq!(query(&b, json!({"subject": "lunch"})).await, [bob]);
    assert!(query(&b, json!({"subject": "holiday"})).await.is_empty());
}

#[tokio::test]
async fn max_size_condition() {
    let (b, alice, bob) = setup().await;
    let got = handle_email_get(
        &b,
        &(),
        json!({"accountId": ACCOUNT, "ids": [&alice, &bob], "properties": ["size"]}),
        &[],
    )
    .await
    .unwrap();
    let size = |i: usize| got.primary["list"][i]["size"].as_u64().unwrap();
    let (small, large) = (size(0), size(1));
    assert!(small < large, "{small} {large}");
    // "less than": an Email of exactly maxSize does not match.
    assert_eq!(
        query(&b, json!({"maxSize": large})).await,
        std::slice::from_ref(&alice)
    );
    assert_eq!(
        query(&b, json!({"maxSize": large + 1})).await,
        sorted(vec![alice.clone(), bob.clone()])
    );
    assert!(query(&b, json!({"maxSize": small})).await.is_empty());
}

#[tokio::test]
async fn body_and_text_conditions() {
    let (b, alice, bob) = setup().await;
    assert_eq!(
        query(&b, json!({"body": "LONG BODY"})).await,
        std::slice::from_ref(&bob)
    );
    assert_eq!(query(&b, json!({"text": "quarterly"})).await, [alice]);
    assert!(query(&b, json!({"body": "quarterly"})).await.is_empty());
}

/// RFC 8621 §4.2 fetchTextBodyValues: "each part in textBody" gets its
/// bodyValues entry. The memory backend now stores the decoded text.
#[tokio::test]
async fn get_fetches_text_body_values() {
    let (b, alice, _) = setup().await;
    let got = handle_email_get(
        &b,
        &(),
        json!({"accountId": ACCOUNT, "ids": [&alice],
               "properties": ["textBody", "bodyValues"], "fetchTextBodyValues": true}),
        &[],
    )
    .await
    .unwrap();
    let email = &got.primary["list"][0];
    let part = email["textBody"][0]["partId"].as_str().unwrap();
    assert_eq!(email["bodyValues"][part]["value"], "short", "{email}");
    // Without the flag, no values (RFC 8621 §4.2 default false).
    let got = handle_email_get(
        &b,
        &(),
        json!({"accountId": ACCOUNT, "ids": [&alice], "properties": ["bodyValues"]}),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(got.primary["list"][0]["bodyValues"], json!({}));
}

/// RFC 8620 §1.4 / §5.5: a `before` / `after` that is not a UTCDate is
/// invalidArguments, even with no Email to evaluate; a non-ASCII value used
/// to panic.
#[tokio::test]
async fn malformed_date_is_invalid_arguments() {
    let b = MemoryBackend::<()>::default();
    b.register_account(&Id::from(ACCOUNT));
    for filter in [
        json!({"before": "202\u{e9}-01-02T03:04:05Z"}),
        json!({"after": "2026-01-02"}),
    ] {
        let err = handle_email_query(
            &b,
            &(),
            json!({"accountId": ACCOUNT, "filter": filter.clone()}),
            &[],
        )
        .await
        .expect_err("malformed date");
        assert_eq!(err.error_type.to_string(), "invalidArguments", "{filter}");
    }
}

/// RFC 8621 §4.1.1: keywords are case-insensitive.
#[tokio::test]
async fn keyword_condition_ignores_case() {
    let (b, alice, bob) = setup().await;
    handle_email_set(
        &b,
        &(),
        &BTreeMap::new(),
        json!({"accountId": ACCOUNT, "update": {alice.clone(): {"keywords/$flagged": true}}}),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(
        query(&b, json!({"hasKeyword": "$FLAGGED"})).await,
        vec![alice]
    );
    assert_eq!(
        query(&b, json!({"notKeyword": "$Flagged"})).await,
        vec![bob]
    );
}
