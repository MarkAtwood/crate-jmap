//! RFC 8621 §5.1 `SearchSnippet/get` `filter` argument shape (bd:JMAP-ytux).
//!
//! RFC 8621 §5.1 types the argument as
//! "filter: FilterOperator|FilterCondition|null", both of which are JSON
//! objects, while RFC 8620 §5.5 reserves "unsupportedFilter" for a filter
//! that "is syntactically valid, but the server cannot process it". A
//! non-object `filter` is therefore `invalidArguments` (RFC 8620 §3.6.2),
//! not `unsupportedFilter`. The oracle is the RFC error type string; the
//! handler under test is never its own oracle.

#![allow(async_fn_in_trait)]

mod common;

use common::MemoryBackend;
use jmap_mail_server::handle_search_snippet_get;
use jmap_types::Id;
use serde_json::{Value, json};

fn one_account_backend() -> (MemoryBackend, Id) {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");
    backend.register_account(&account_id);
    (backend, account_id)
}

async fn call_with_filter(filter: Value) -> Result<Value, String> {
    let (backend, account_id) = one_account_backend();
    let args = json!({
        "accountId": account_id.as_ref(),
        "emailIds": ["e1"],
        "filter": filter,
    });
    handle_search_snippet_get(&backend, &(), args)
        .await
        .map(|out| out.primary)
        .map_err(|e| e.error_type.to_string())
}

/// RFC 8621 §5.1: "filter: FilterOperator|FilterCondition|null". Neither a
/// number, a string nor an array is an object, so each is invalidArguments.
#[tokio::test]
async fn non_object_filter_is_invalid_arguments() {
    for filter in [json!(42), json!("text"), json!([]), json!(true)] {
        let err = call_with_filter(filter.clone())
            .await
            .expect_err("a non-object filter must be rejected");
        assert_eq!(err, "invalidArguments", "filter = {filter}");
    }
}

/// RFC 8620 §5.5: ""unsupportedFilter": The "filter" is syntactically
/// valid, but the server cannot process it." An object whose condition
/// property has the wrong type stays unsupportedFilter.
#[tokio::test]
async fn object_filter_that_fails_to_decode_is_unsupported_filter() {
    let err = call_with_filter(json!({ "inMailbox": 42 }))
        .await
        .expect_err("an undecodable filter object must be rejected");
    assert_eq!(err, "unsupportedFilter");
}

/// RFC 8621 §5.1: `filter` may be `null`; the call is then processed and
/// the requested id is answered exactly once, in `list` or in `notFound`.
#[tokio::test]
async fn null_filter_is_processed() {
    let resp = call_with_filter(Value::Null)
        .await
        .expect("a null filter must be accepted");
    let answered: Vec<Value> = resp["list"]
        .as_array()
        .expect("list must be an array")
        .iter()
        .map(|s| s["emailId"].clone())
        .chain(
            resp["notFound"]
                .as_array()
                .expect("notFound must be an array")
                .iter()
                .cloned(),
        )
        .collect();
    assert_eq!(answered, vec![json!("e1")]);
}
