//! Email/parse and Thread/get `properties`.
//!
//! Oracles: RFC 8621 §4.9 "The following metadata properties on the Email
//! objects will be null if requested: id, mailboxIds, keywords,
//! receivedAt" (blobId is not among them); RFC 8620 §5.1 "If an invalid
//! property is requested, the call MUST be rejected with an
//! "invalidArguments" error".

use jmap_mail_server::memory::MemoryBackend;
use jmap_mail_server::{handle_email_parse, handle_thread_get};
use jmap_types::Id;
use serde_json::json;

fn backend() -> MemoryBackend<()> {
    let b = MemoryBackend::<()>::default();
    b.register_account(&Id::from("acc"));
    b.store_blob(
        &Id::from("B1"),
        b"From: a@example.com\r\nSubject: s\r\n\r\nbody\r\n".to_vec(),
    );
    b
}

#[tokio::test]
async fn parse_returns_the_blob_id_and_no_unrequested_id() {
    let b = backend();
    let resp = handle_email_parse(
        &b,
        &(),
        json!({"accountId": "acc", "blobIds": ["B1"], "properties": ["blobId", "keywords", "subject"]}),
    )
    .await
    .unwrap()
    .primary;
    assert_eq!(
        resp["parsed"]["B1"],
        json!({"blobId": "B1", "keywords": null, "subject": "s"}),
        "{resp}"
    );
}

#[tokio::test]
async fn parse_rejects_unknown_properties() {
    let b = backend();
    let err = handle_email_parse(
        &b,
        &(),
        json!({"accountId": "acc", "blobIds": ["B1"], "properties": ["nonsense"]}),
    )
    .await
    .expect_err("unknown property");
    assert_eq!(err.error_type.to_string(), "invalidArguments");
}

#[tokio::test]
async fn thread_get_rejects_unknown_properties() {
    let b = backend();
    let err = handle_thread_get(
        &b,
        &(),
        json!({"accountId": "acc", "properties": ["subject"]}),
    )
    .await
    .expect_err("unknown property");
    assert_eq!(err.error_type.to_string(), "invalidArguments");
}
