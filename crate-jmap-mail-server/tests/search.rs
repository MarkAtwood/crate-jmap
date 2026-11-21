//! JMAP Search `where` on `Email/query` and `Email/queryChanges`.
//!
//! Oracle: crate-jmap docs/specs/jmap-search.md (draft-atwood-jmap-search-00),
//! cited by section, with the expected ids worked out by hand from the
//! Emails created below.

mod common;

use std::collections::BTreeMap;

use jmap_mail_server::memory::MemoryBackend;
use jmap_mail_server::{handle_email_query, handle_email_query_changes, handle_email_set};
use jmap_search::{DataTypeSearchInfo, JMAP_SEARCH_URI, SearchCapability};
use jmap_types::Id;
use serde_json::{Value, json};

const ACCOUNT: &str = "account1";

fn using() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:mail".to_owned(),
        JMAP_SEARCH_URI.to_owned(),
    ]
}

fn email_search(max_depth: u64, supports_text: bool) -> SearchCapability {
    SearchCapability::new(max_depth).with_data_type("Email", DataTypeSearchInfo::new(supports_text))
}

async fn create(b: &MemoryBackend<()>, subject: &str, body: &str, flagged: bool) -> String {
    let keywords = if flagged {
        json!({"$flagged": true})
    } else {
        json!({})
    };
    let out = handle_email_set(
        b,
        &(),
        &BTreeMap::new(),
        json!({"accountId": ACCOUNT, "create": {"c": {
            "mailboxIds": {"M1": true},
            "keywords": keywords,
            "from": [{"name": "Alice", "email": "alice@example.com"}],
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

/// Three Emails: a large flagged invoice, a small flagged invoice and a
/// large unflagged one.
async fn setup(cap: Option<SearchCapability>) -> (MemoryBackend<()>, [String; 3]) {
    let mut b = MemoryBackend::<()>::default();
    if let Some(cap) = cap {
        b = b.with_search_capability(cap);
    }
    b.register_account(&Id::from(ACCOUNT));
    common::seed_mailboxes(&b, &Id::from(ACCOUNT), &["M1"]);
    let big = "x".repeat(1_100_000);
    let large_flagged = create(&b, "INVOICE 2026-11", &big, true).await;
    let small_flagged = create(&b, "Invoice 2026-10", "short note", true).await;
    let large_plain = create(&b, "Invoice 2026-09", &big, false).await;
    (b, [large_flagged, small_flagged, large_plain])
}

async fn query(b: &MemoryBackend<()>, filter: Value, using: &[String]) -> Result<Value, String> {
    handle_email_query(
        b,
        &(),
        json!({"accountId": ACCOUNT, "filter": filter}),
        using,
    )
    .await
    .map(|o| o.primary)
    .map_err(|e| e.error_type.to_string())
}

fn ids(resp: &Value) -> Vec<String> {
    let mut v: Vec<String> = resp["ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    v.sort();
    v
}

/// §Examples "Large flagged Emails", the filter copied byte-for-byte.
#[tokio::test]
async fn large_flagged_example() {
    let (b, [large_flagged, ..]) = setup(Some(email_search(4, true))).await;
    let filter: Value = serde_json::from_str(
        r#"{
    "inMailbox": "M1",
    "where": {
      "/keywords/$flagged": true,
      "/size": { "$gt": 1000000 },
      "/subject": {
        "$prefix": "invoice",
        "$collation": "i;unicode-casemap"
      }
    }
  }"#,
    )
    .unwrap();
    let resp = query(&b, filter, &using()).await.unwrap();
    assert_eq!(ids(&resp), [large_flagged]);
    // §Query Changes: "SHOULD return `canCalculateChanges: false` from the
    // `/query` with that filter".
    assert_eq!(resp["canCalculateChanges"], json!(false));
}

/// §Opting In a Data Type: "A data type accepts `where` on an account
/// exactly when the server lists it in that account's `dataTypes`, and only
/// in requests whose `using` includes `urn:ietf:params:jmap:search`";
/// §Errors: otherwise `unsupportedFilter`.
#[tokio::test]
async fn where_needs_capability_and_using() {
    let filter = json!({"where": {"/size": {"$gt": 0}}});
    let (b, _) = setup(None).await;
    assert_eq!(
        query(&b, filter.clone(), &using()).await.unwrap_err(),
        "unsupportedFilter"
    );

    let other_type =
        SearchCapability::new(4).with_data_type("Mailbox", DataTypeSearchInfo::new(false));
    let (b, _) = setup(Some(other_type)).await;
    assert_eq!(
        query(&b, filter.clone(), &using()).await.unwrap_err(),
        "unsupportedFilter"
    );

    let (b, _) = setup(Some(email_search(4, false))).await;
    let no_search = vec!["urn:ietf:params:jmap:core".to_owned()];
    assert_eq!(
        query(&b, filter.clone(), &no_search).await.unwrap_err(),
        "unsupportedFilter"
    );
    assert_eq!(ids(&query(&b, filter, &using()).await.unwrap()).len(), 3);

    // A filter without `where` is unaffected and keeps canCalculateChanges.
    let resp = query(&b, json!({"inMailbox": "M1"}), &no_search)
        .await
        .unwrap();
    assert_eq!(resp["canCalculateChanges"], json!(true));
}

/// §Errors: `invalidArguments` for a malformed `where`, `unsupportedFilter`
/// for an unknown `$` name or `$text` without `supportsText`,
/// `queryTooComplex` past `maxDepth`.
#[tokio::test]
async fn error_types() {
    let (b, _) = setup(Some(email_search(1, false))).await;
    let cases = [
        (json!({"subject": "x"}), "invalidArguments"),
        (json!({"$regex": "x"}), "unsupportedFilter"),
        (json!({"$text": "x"}), "unsupportedFilter"),
        (
            json!({"/subject": {"$eq": "x", "$collation": "i;x-none"}}),
            "unsupportedFilter",
        ),
        (json!({"$not": {"$not": {}}}), "queryTooComplex"),
    ];
    for (w, want) in cases {
        let got = query(&b, json!({"where": w}), &using()).await.unwrap_err();
        assert_eq!(got, want, "{w}");
    }
    assert!(
        query(&b, json!({"where": {"$not": {}}}), &using())
            .await
            .is_ok()
    );
}

/// §The where FilterCondition Property: a condition with `where` "may
/// appear anywhere the type's `/query` accepts a FilterCondition, including
/// inside FilterOperator objects"; §Logical Operators.
#[tokio::test]
async fn where_inside_filter_operators() {
    let (b, [large_flagged, small_flagged, large_plain]) =
        setup(Some(email_search(4, false))).await;
    let filter = json!({
        "operator": "NOT",
        "conditions": [{"where": {"/keywords/$flagged": true}}]
    });
    assert_eq!(
        ids(&query(&b, filter, &using()).await.unwrap()),
        std::slice::from_ref(&large_plain)
    );
    let filter = json!({
        "operator": "OR",
        "conditions": [
            {"where": {"/size": {"$lt": 1000}}},
            {"hasKeyword": "$flagged", "where": {"/subject": {"$contains": "2026-11"}}}
        ]
    });
    let mut want = vec![large_flagged, small_flagged];
    want.sort();
    assert_eq!(ids(&query(&b, filter, &using()).await.unwrap()), want);
}

/// §Addressable Properties: pointers into arrays of objects
/// (`/from/0/email`), and "`bodyValues` of an Email is empty because
/// `fetchTextBodyValues` defaults to `false`"; §Full-Text Matching:
/// `$text` "SHOULD match as" `Email/query` `text`, which searches text body
/// parts (RFC 8621 §4.4.1).
#[tokio::test]
async fn addressing_and_text() {
    let (b, [large_flagged, small_flagged, large_plain]) = setup(Some(email_search(4, true))).await;
    let all = {
        let mut v = vec![large_flagged, small_flagged.clone(), large_plain];
        v.sort();
        v
    };
    let using = using();
    let run = |w: Value| query(&b, json!({"where": w}), &using);
    let got = run(json!({"/from/0/email": "alice@example.com"})).await;
    assert_eq!(ids(&got.unwrap()), all);
    let got = run(json!({"/bodyValues/1/value": {"$exists": true}})).await;
    assert!(ids(&got.unwrap()).is_empty());
    let got = run(json!({"$text": "SHORT NOTE"})).await;
    assert_eq!(ids(&got.unwrap()), [small_flagged]);
}

/// §Query Changes: "Servers MAY return `cannotCalculateChanges` for any
/// `/queryChanges` call whose filter uses `where`".
#[tokio::test]
async fn query_changes_with_where() {
    let (b, _) = setup(Some(email_search(4, false))).await;
    let call = |filter: Value| {
        handle_email_query_changes(
            &b,
            &(),
            json!({"accountId": ACCOUNT, "sinceQueryState": "0", "filter": filter}),
            &[],
        )
    };
    // Without `using` the filter is refused before anything else.
    let e = call(json!({"where": {}})).await.unwrap_err();
    assert_eq!(e.error_type.to_string(), "unsupportedFilter");
    let e = handle_email_query_changes(
        &b,
        &(),
        json!({"accountId": ACCOUNT, "sinceQueryState": "0", "filter": {"where": {}}}),
        &using(),
    )
    .await
    .unwrap_err();
    assert_eq!(e.error_type.to_string(), "cannotCalculateChanges");
}
