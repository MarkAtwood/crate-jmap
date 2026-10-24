//! The in-memory backend rejects draft-ietf-jmap-metadata-04 §3.6 filter
//! conditions on Email and Mailbox with `unsupportedFilter` unless
//! `with_metadata_info` gave the type metadata (bd JMAP-xdp4; evaluation
//! for opted-in types is in `metadata.rs`, JMAP-s4kp.1). -04 §1.2.1: an
//! unlisted type "does not gain the metadata or privateMetadata
//! properties".
//!
//! Oracle: RFC 8620 §5.5 (https://www.rfc-editor.org/rfc/rfc8620.txt):
//!
//! > "unsupportedFilter": The "filter" is syntactically valid, but the
//! > server cannot process it.
//!
//! The filters are hand-written; the `privateMetadataExists` leaf is the
//! one in the draft-ietf-jmap-metadata-02 §6.7 example request. A leaf
//! nested under an operator must be rejected too, not matched as if the
//! condition were absent.

mod common;

use common::MemoryBackend;
use jmap_mail_server::{handle_email_query, handle_mailbox_query};
use jmap_types::Id;
use serde_json::{Value, json};

const ACCOUNT_ID: &str = "acct1";
/// The requests use the capability, so the rejection is the unlisted
/// type's, not the -04 §3 one for a request without it.
fn using() -> Vec<String> {
    vec!["urn:ietf:params:jmap:metadata".to_owned()]
}

fn backend() -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend
}

fn error_type(err: jmap_types::JmapError) -> Value {
    serde_json::to_value(&err).expect("JmapError must serialize")["type"].clone()
}

#[tokio::test]
async fn email_query_rejects_nested_metadata_condition() {
    let args = json!({
        "accountId": ACCOUNT_ID,
        "filter": {
            "operator": "AND",
            "conditions": [
                { "inMailbox": "MB-inbox" },
                { "privateMetadataExists": "acme.example.com/memo" }
            ]
        }
    });
    let err = handle_email_query(&backend(), &(), args, &using())
        .await
        .expect_err("a metadata condition must fail Email/query");
    assert_eq!(error_type(err), json!("unsupportedFilter"));
}

#[tokio::test]
async fn mailbox_query_rejects_metadata_condition() {
    let args = json!({
        "accountId": ACCOUNT_ID,
        "filter": { "metadataExists": "acme.example.com" }
    });
    let err = handle_mailbox_query(&backend(), &(), args, &using())
        .await
        .expect_err("a metadata condition must fail Mailbox/query");
    assert_eq!(error_type(err), json!("unsupportedFilter"));
}

#[tokio::test]
async fn email_query_without_metadata_condition_still_succeeds() {
    let args = json!({
        "accountId": ACCOUNT_ID,
        "filter": { "operator": "NOT", "conditions": [{ "hasKeyword": "$seen" }] }
    });
    handle_email_query(&backend(), &(), args, &using())
        .await
        .expect("a filter without metadata conditions must succeed");
}
