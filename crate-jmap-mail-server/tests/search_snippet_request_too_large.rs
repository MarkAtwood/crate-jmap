//! RFC 8621 §5.1 `SearchSnippet/get` `requestTooLarge` enforcement on
//! `emailIds` (bd:JMAP-yt1w.38).
//!
//! The cap is the default `JmapBackend::max_objects_in_get` of 500 used by
//! every other /get handler (bd:JMAP-yt1w.31). The oracle is the RFC error
//! type string and the requested id list; the handler under test is never
//! its own oracle.

#![allow(async_fn_in_trait)]

mod common;

use common::MemoryBackend;
use jmap_mail_server::handle_search_snippet_get;
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{json, Value};

// Default cap value returned by `JmapBackend::max_objects_in_get` in the
// foundation crate.
const DEFAULT_CAP: usize = 500;

/// Build `n` distinct Id-shaped strings that name no stored email.
fn email_ids_of_size(n: usize) -> Vec<Value> {
    (0..n).map(|i| json!(format!("email-{i}"))).collect()
}

fn one_account_backend() -> (MemoryBackend, Id) {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");
    backend.register_account(&account_id);
    (backend, account_id)
}

/// RFC 8621 §5.1 (rfc8621.txt, "SearchSnippet/get"): ""requestTooLarge":
/// The number of "emailIds" requested by the client exceeds the maximum
/// number the server is willing to process in a single method call."
#[tokio::test]
async fn search_snippet_get_over_cap_returns_request_too_large() {
    let (backend, account_id) = one_account_backend();
    let args = json!({
        "accountId": account_id.as_ref(),
        "emailIds": email_ids_of_size(DEFAULT_CAP + 1),
    });
    let err = handle_search_snippet_get(&backend, &(), args)
        .await
        .expect_err("501 emailIds must exceed the maxObjectsInGet cap");
    assert_eq!(err.error_type.to_string(), "requestTooLarge");
}

/// RFC 8621 §5.1 (rfc8621.txt, "SearchSnippet/get"): ""requestTooLarge":
/// The number of "emailIds" requested by the client exceeds the maximum
/// number the server is willing to process in a single method call."
///
/// At-cap positive control: exactly `DEFAULT_CAP` emailIds does not exceed
/// the maximum, so the call is processed: every requested id is accounted
/// for exactly once, either as a `list` entry or in `notFound`.
#[tokio::test]
async fn search_snippet_get_at_cap_is_processed() {
    let (backend, account_id) = one_account_backend();
    let ids = email_ids_of_size(DEFAULT_CAP);
    let args = json!({
        "accountId": account_id.as_ref(),
        "emailIds": ids.clone(),
    });
    let HandlerOutput { primary: resp, .. } = handle_search_snippet_get(&backend, &(), args)
        .await
        .expect("exactly 500 emailIds must not trip the cap");
    let mut answered: Vec<Value> = resp["list"]
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
    let mut expected = ids;
    let key = |v: &Value| v.as_str().unwrap_or_default().to_owned();
    answered.sort_by_key(key);
    expected.sort_by_key(key);
    assert_eq!(answered, expected);
}
