//! fetch*BodyValues select "text/*" parts (RFC 8621 §4.2):
//! fetchTextBodyValues "includes any "text/*" part in the "textBody"
//! property"; fetchAllBodyValues "includes any "text/*" part in the
//! "bodyStructure" property".

use jmap_mail_server::handle_email_parse;
use jmap_mail_server::memory::MemoryBackend;
use jmap_types::Id;
use serde_json::{Value, json};

const MESSAGE: &[u8] = b"From: a@example.com\r\n\
Subject: s\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"b\"\r\n\
\r\n\
--b\r\n\
Content-Type: text/plain\r\n\
\r\n\
Hello.\r\n\
--b\r\n\
Content-Type: image/png\r\n\
Content-Disposition: inline\r\n\
\r\n\
PNGDATA\r\n\
--b\r\n\
Content-Type: text/plain\r\n\
Content-Disposition: attachment; filename=\"notes.txt\"\r\n\
\r\n\
Notes.\r\n\
--b--\r\n";

async fn body_values(flag: &str) -> (Value, Value) {
    let b = MemoryBackend::<()>::default();
    b.register_account(&Id::from("acc"));
    b.store_blob(&Id::from("B1"), MESSAGE.to_vec());
    let resp = handle_email_parse(
        &b,
        &(),
        json!({"accountId": "acc", "blobIds": ["B1"],
               "properties": ["bodyValues", "textBody", "bodyStructure"], flag: true}),
    )
    .await
    .unwrap()
    .primary;
    let email = resp["parsed"]["B1"].clone();
    (email["bodyValues"].clone(), email)
}

/// The values of `bodyValues`, sorted.
fn values(bv: &Value) -> Vec<String> {
    let mut v: Vec<String> = bv
        .as_object()
        .unwrap()
        .values()
        .map(|e| e["value"].as_str().unwrap().trim().to_owned())
        .collect();
    v.sort();
    v
}

#[tokio::test]
async fn text_body_values_skip_non_text_parts() {
    let (bv, email) = body_values("fetchTextBodyValues").await;
    assert_eq!(values(&bv), ["Hello."], "{email}");
}

#[tokio::test]
async fn all_body_values_include_text_attachments() {
    let (bv, email) = body_values("fetchAllBodyValues").await;
    assert_eq!(values(&bv), ["Hello.", "Notes."], "{email}");
}
