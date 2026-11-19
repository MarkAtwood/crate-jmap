//! Email/get rejects unknown property names (bd JMAP-r44n.15).
//!
//! Oracle: RFC 8620 §5.1, "If an invalid property is requested, the call
//! MUST be rejected with an "invalidArguments" error"; the valid names are
//! RFC 8621 §4.1's Email properties and §4.1.3's `header:` forms.

use jmap_mail_server::handle_email_get;
use jmap_mail_server::memory::MemoryBackend;
use jmap_types::{Id, MethodErrorType};
use serde_json::json;

async fn get(properties: serde_json::Value) -> Result<serde_json::Value, jmap_types::JmapError> {
    let b = MemoryBackend::new();
    b.register_account(&Id::from("a"));
    handle_email_get(
        &b,
        &(),
        json!({"accountId": "a", "ids": [], "properties": properties}),
        &[],
    )
    .await
    .map(|o| o.primary)
}

#[tokio::test]
async fn unknown_name_is_invalid_arguments() {
    for props in [
        json!(["nope"]),
        json!(["subject", "nope"]),
        json!(["objectHistory"]),
    ] {
        let err = get(props.clone()).await.unwrap_err();
        assert_eq!(err.error_type, MethodErrorType::InvalidArguments, "{props}");
    }
}

/// Without metadata for Email, `metadata` is not an Email property here.
#[tokio::test]
async fn metadata_needs_the_capability_on_email() {
    let err = get(json!(["metadata"])).await.unwrap_err();
    assert_eq!(err.error_type, MethodErrorType::InvalidArguments);
}

#[tokio::test]
async fn every_rfc_8621_property_is_accepted() {
    let all = json!([
        "id",
        "blobId",
        "threadId",
        "mailboxIds",
        "keywords",
        "size",
        "receivedAt",
        "messageId",
        "inReplyTo",
        "references",
        "sender",
        "from",
        "to",
        "cc",
        "bcc",
        "replyTo",
        "subject",
        "sentAt",
        "hasAttachment",
        "preview",
        "bodyValues",
        "textBody",
        "htmlBody",
        "attachments",
        "headers",
        "bodyStructure",
        "header:List-Id",
        "header:From:asAddresses:all"
    ]);
    get(all).await.unwrap();
}
