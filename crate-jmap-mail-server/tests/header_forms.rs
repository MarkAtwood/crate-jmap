//! `header:{name}:as{Form}` properties are checked against RFC 8621
//! §4.1.2: each form "may only be fetched or set for" the fields its
//! subsection lists and "Any header field not defined in [RFC5322] or
//! [RFC2369]"; §4.2: a forbidden form "MUST result in the method call
//! being rejected with an "invalidArguments" error".

use jmap_mail_server::handle_email_parse;
use jmap_mail_server::memory::MemoryBackend;
use jmap_types::Id;
use serde_json::json;

async fn parse(property: &str) -> Result<(), String> {
    let b = MemoryBackend::<()>::default();
    b.register_account(&Id::from("acc"));
    b.store_blob(
        &Id::from("B1"),
        b"From: a@example.com\r\nSubject: s\r\nReceived: from x\r\nX-Custom: 1\r\n\r\nbody\r\n"
            .to_vec(),
    );
    handle_email_parse(
        &b,
        &(),
        json!({"accountId": "acc", "blobIds": ["B1"], "properties": [property]}),
    )
    .await
    .map(|_| ())
    .map_err(|e| e.error_type.to_string())
}

#[tokio::test]
async fn forbidden_forms_are_invalid_arguments() {
    for p in [
        "header:From:asText",
        "header:Subject:asAddresses",
        "header:Received:asDate",
        "header:Received:asText",
        "header:Date:asAddresses",
        "header:List-Post:asText",
        "header:Message-ID:asText",
    ] {
        assert_eq!(parse(p).await, Err("invalidArguments".to_owned()), "{p}");
    }
}

#[tokio::test]
async fn permitted_forms_are_accepted() {
    for p in [
        "header:Subject:asText",
        "header:From:asGroupedAddresses",
        "header:Received",
        "header:X-Custom:asDate",
        "header:X-Custom:asAddresses",
        "header:List-Id:asText",
        "header:Comments:asText",
    ] {
        assert_eq!(parse(p).await, Ok(()), "{p}");
    }
}

/// RFC 8621 §4.1.2.1 Raw: "The raw octets of the header field value from
/// the first octet following the header field name terminating colon, up
/// to but excluding the header field terminating CRLF": a folding CRLF
/// stays. §4.1.3 `headers` values are in Raw form too.
#[tokio::test]
async fn raw_form_keeps_folding_crlf() {
    let b = MemoryBackend::<()>::default();
    b.register_account(&Id::from("acc"));
    b.store_blob(
        &Id::from("B1"),
        b"From: a@example.com\r\nX-Folded: one\r\n two\r\n\r\nbody\r\n".to_vec(),
    );
    let resp = handle_email_parse(
        &b,
        &(),
        json!({"accountId": "acc", "blobIds": ["B1"],
               "properties": ["header:X-Folded", "header:X-Folded:asText", "headers"]}),
    )
    .await
    .unwrap()
    .primary;
    let email = &resp["parsed"]["B1"];
    assert_eq!(email["header:X-Folded"], " one\r\n two", "{resp}");
    assert_eq!(email["header:X-Folded:asText"], "one two", "{resp}");
    let folded = email["headers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["name"] == "X-Folded")
        .unwrap();
    assert_eq!(folded["value"], " one\r\n two", "{resp}");
}

/// RFC 8621 §4.1.2.2 Text: encoded-words with a known charset are decoded
/// and the result is NFC, as for the `subject` property.
#[tokio::test]
async fn text_form_decodes_encoded_words() {
    let b = MemoryBackend::<()>::default();
    b.register_account(&Id::from("acc"));
    b.store_blob(
        &Id::from("B1"),
        b"From: a@example.com\r\nSubject: =?UTF-8?Q?Caf=C3=A9?=\r\n\r\nbody\r\n".to_vec(),
    );
    let resp = handle_email_parse(
        &b,
        &(),
        json!({"accountId": "acc", "blobIds": ["B1"],
               "properties": ["subject", "header:Subject:asText"]}),
    )
    .await
    .unwrap()
    .primary;
    let email = &resp["parsed"]["B1"];
    assert_eq!(email["header:Subject:asText"], "Caf\u{e9}", "{resp}");
    assert_eq!(email["header:Subject:asText"], email["subject"], "{resp}");
}
