//! `Email/set` create stores the RFC 5322 message the create describes
//! (bd:JMAP-u6c5).
//!
//! Oracles: RFC 8621 §4.1.1 (`blobId` is "The id representing the raw
//! octets of the message [RFC5322] for this Email"; `size` is "The size,
//! in octets, of the raw data for the message"), RFC 8621 §4.6 ("If not
//! included, the server MUST generate and set a Message-ID header field
//! ... and a Date header field"; `blobNotFound`: "An extra "notFound"
//! property of type "Id[]" MUST be included in the SetError object
//! containing every "blobId" referenced by an EmailBodyPart that could not
//! be found on the server"), and hand-written RFC 5322 messages. `Q2Fmw6k=`
//! is Python `base64.b64encode("Café".encode())` and `JVBERi0x` is
//! Python `base64.b64encode(b"%PDF-1")`.

#![allow(async_fn_in_trait)]

mod common;

use std::collections::BTreeMap;

use common::MemoryBackend;
use jmap_mail_server::{
    MailBackend, handle_email_get, handle_email_set, handle_mailbox_get, handle_thread_get,
};
use jmap_server::{HandlerOutput, JmapBackend};
use jmap_types::Id;
use serde_json::{Value, json};

fn backend() -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from("a1"));
    backend
}

async fn set(backend: &MemoryBackend, create: Value) -> Value {
    let args = json!({ "accountId": "a1", "create": { "c0": create } });
    let HandlerOutput { primary, .. } = handle_email_set(backend, &(), &BTreeMap::new(), args, &[])
        .await
        .expect("Email/set must succeed");
    primary
}

async fn get(backend: &MemoryBackend, id: &str, properties: &[&str]) -> Value {
    let args = json!({ "accountId": "a1", "ids": [id], "properties": properties });
    let HandlerOutput { primary, .. } = handle_email_get(backend, &(), args, &[])
        .await
        .expect("Email/get must succeed");
    primary["list"][0].clone()
}

async fn stored_message(backend: &MemoryBackend, blob_id: &str) -> String {
    let bytes = MailBackend::read_blob(backend, &(), &Id::from("a1"), &Id::from(blob_id))
        .await
        .unwrap()
        .expect("blobId must name a stored blob");
    String::from_utf8(bytes).unwrap()
}

#[tokio::test]
async fn create_stores_composed_message_and_reads_it_back() {
    let backend = backend();
    let resp = set(
        &backend,
        json!({
            "mailboxIds": { "inbox": true },
            "from": [{ "name": "Alice", "email": "alice@example.com" }],
            "to": [{ "email": "bob@example.com" }],
            "subject": "Café",
            "sentAt": "2026-10-23T12:00:00Z",
            "messageId": ["m1@example.com"],
            "textBody": [{ "partId": "1", "type": "text/plain" }],
            "bodyValues": { "1": { "value": "Noon?" } },
        }),
    )
    .await;
    let created = &resp["created"]["c0"];
    let expected = "Date: 23 Oct 2026 12:00:00 +0000\r\n\
                    From: Alice <alice@example.com>\r\n\
                    To: bob@example.com\r\n\
                    Message-ID: <m1@example.com>\r\n\
                    Subject: =?UTF-8?B?Q2Fmw6k=?=\r\n\
                    MIME-Version: 1.0\r\n\
                    Content-Type: text/plain; charset=utf-8\r\n\
                    Content-Transfer-Encoding: quoted-printable\r\n\
                    \r\n\
                    Noon?";
    let blob_id = created["blobId"]
        .as_str()
        .expect("created must carry blobId");
    assert_eq!(stored_message(&backend, blob_id).await, expected);
    assert_eq!(created["size"], json!(expected.len()));

    let email = get(
        &backend,
        created["id"].as_str().unwrap(),
        &[
            "blobId",
            "size",
            "from",
            "to",
            "subject",
            "sentAt",
            "messageId",
            "textBody",
            "preview",
        ],
    )
    .await;
    assert_eq!(email["blobId"], json!(blob_id));
    assert_eq!(email["size"], json!(expected.len()));
    assert_eq!(
        email["from"],
        json!([{ "name": "Alice", "email": "alice@example.com" }])
    );
    assert_eq!(email["to"][0]["email"], json!("bob@example.com"));
    assert_eq!(email["subject"], json!("Café"));
    assert_eq!(email["sentAt"], json!("2026-10-23T12:00:00Z"));
    assert_eq!(email["messageId"], json!(["m1@example.com"]));
    assert_eq!(email["textBody"][0]["type"], json!("text/plain"));
    assert_eq!(email["preview"], json!("Noon?"));
}

#[tokio::test]
async fn create_writes_blob_parts_into_message() {
    let backend = backend();
    backend.store_blob(&Id::from("pdf1"), b"%PDF-1".to_vec());
    let resp = set(
        &backend,
        json!({
            "mailboxIds": { "inbox": true },
            "sentAt": "2026-10-23T12:00:00Z",
            "messageId": ["m2@example.com"],
            "textBody": [{ "partId": "t", "type": "text/plain" }],
            "attachments": [{
                "blobId": "pdf1",
                "type": "application/pdf",
                "name": "a.pdf",
                "disposition": "attachment",
            }],
            "bodyValues": { "t": { "value": "See attached." } },
        }),
    )
    .await;
    let created = &resp["created"]["c0"];
    assert_eq!(
        stored_message(&backend, created["blobId"].as_str().unwrap()).await,
        "Date: 23 Oct 2026 12:00:00 +0000\r\n\
         Message-ID: <m2@example.com>\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"=_0\"\r\n\
         \r\n\
         --=_0\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         Content-Transfer-Encoding: quoted-printable\r\n\
         \r\n\
         See attached.\r\n\
         --=_0\r\n\
         Content-Type: application/pdf; name=a.pdf\r\n\
         Content-Disposition: attachment; filename=a.pdf\r\n\
         Content-Transfer-Encoding: base64\r\n\
         \r\n\
         JVBERi0x\r\n\
         --=_0--\r\n"
    );

    let email = get(
        &backend,
        created["id"].as_str().unwrap(),
        &["hasAttachment", "attachments"],
    )
    .await;
    assert_eq!(email["hasAttachment"], json!(true));
    assert_eq!(email["attachments"][0]["type"], json!("application/pdf"));
    assert_eq!(email["attachments"][0]["name"], json!("a.pdf"));
}

#[tokio::test]
async fn create_without_message_id_or_sent_at_gets_both() {
    let backend = backend();
    let resp = set(
        &backend,
        json!({
            "mailboxIds": { "inbox": true },
            "from": [{ "email": "alice@example.com" }],
            "receivedAt": "2026-10-23T10:00:00Z",
        }),
    )
    .await;
    let created = &resp["created"]["c0"];
    let message = stored_message(&backend, created["blobId"].as_str().unwrap()).await;
    assert!(
        message.starts_with("Date: 23 Oct 2026 10:00:00 +0000\r\n"),
        "{message}"
    );

    let email = get(
        &backend,
        created["id"].as_str().unwrap(),
        &["messageId", "sentAt"],
    )
    .await;
    assert_eq!(email["sentAt"], json!("2026-10-23T10:00:00Z"));
    let ids = email["messageId"]
        .as_array()
        .expect("messageId must be set");
    assert_eq!(ids.len(), 1);
    assert!(
        ids[0].as_str().unwrap().ends_with("@example.com"),
        "{ids:?}"
    );
}

#[tokio::test]
async fn create_with_missing_blobs_reports_every_one() {
    let backend = backend();
    let resp = set(
        &backend,
        json!({
            "mailboxIds": { "inbox": true },
            "attachments": [
                { "blobId": "nope1", "type": "application/octet-stream" },
                { "blobId": "nope2", "type": "application/octet-stream" },
            ],
        }),
    )
    .await;
    let err = &resp["notCreated"]["c0"];
    assert_eq!(err["type"], json!("blobNotFound"));
    assert_eq!(err["notFound"], json!(["nope1", "nope2"]));
    assert!(resp["created"].as_object().is_none_or(|m| m.is_empty()));
}

/// A line break in an address would end the To field and start a new one.
#[tokio::test]
async fn create_rejects_header_injection() {
    let backend = backend();
    let resp = set(
        &backend,
        json!({
            "mailboxIds": { "inbox": true },
            "to": [{ "email": "bob@example.com\r\nBcc: eve@example.com" }],
        }),
    )
    .await;
    assert_eq!(resp["notCreated"]["c0"]["type"], json!("invalidProperties"));
    assert!(resp["created"].as_object().is_none_or(|m| m.is_empty()));
}

/// RFC 8621 §3: "Every email belongs to a Thread"; `emailIds` is "The ids
/// of the Emails in the Thread, sorted by the receivedAt date of the Email,
/// oldest first". RFC 8621 §2: `totalEmails` is "The number of Emails in
/// this Mailbox", `unreadEmails` those "that have neither the $seen
/// keyword nor the $draft keyword", `totalThreads` "The number of Threads
/// where at least one Email in the Thread is in this Mailbox", and
/// `unreadThreads` those with an unread Email in it. An `Email/set` create
/// must maintain all of these as `Email/import` does (bd:JMAP-p36f).
#[tokio::test]
async fn create_maintains_thread_and_mailbox_counts() {
    let backend = backend();
    let account = Id::from("a1");
    backend.seed_object(
        &account,
        "Mailbox",
        &Id::from("inbox"),
        json!({
            "id": "inbox", "name": "Inbox", "parentId": null, "role": "inbox",
            "sortOrder": 0, "totalEmails": 0, "unreadEmails": 0,
            "totalThreads": 0, "unreadThreads": 0,
            "myRights": {
                "mayReadItems": true, "mayAddItems": true, "mayRemoveItems": true,
                "maySetSeen": true, "maySetKeywords": true, "mayCreateChild": true,
                "mayRename": true, "mayDelete": true, "maySubmit": true
            },
            "isSubscribed": true,
        }),
    );
    let thread_state_before = backend
        .get_state::<jmap_mail_types::Thread>(&(), &account)
        .await
        .unwrap();

    let first = set(
        &backend,
        json!({
            "mailboxIds": { "inbox": true },
            "messageId": ["root@example.com"],
            "receivedAt": "2026-10-23T10:00:00Z",
            "subject": "Lunch",
        }),
    )
    .await;
    let first = &first["created"]["c0"];
    let reply = set(
        &backend,
        json!({
            "mailboxIds": { "inbox": true },
            "keywords": { "$seen": true },
            "messageId": ["reply@example.com"],
            "inReplyTo": ["root@example.com"],
            "receivedAt": "2026-10-23T11:00:00Z",
            "subject": "Re: Lunch",
        }),
    )
    .await;
    let reply = &reply["created"]["c0"];
    assert_eq!(reply["threadId"], first["threadId"]);

    let args = json!({ "accountId": "a1", "ids": [first["threadId"]] });
    let HandlerOutput { primary, .. } = handle_thread_get(&backend, &(), args)
        .await
        .expect("Thread/get must succeed");
    assert_eq!(
        primary["list"],
        json!([{ "id": first["threadId"], "emailIds": [first["id"], reply["id"]] }])
    );
    assert_ne!(primary["state"], json!(thread_state_before.as_ref()));

    let args = json!({
        "accountId": "a1",
        "ids": ["inbox"],
        "properties": ["totalEmails", "unreadEmails", "totalThreads", "unreadThreads"],
    });
    let HandlerOutput { primary, .. } = handle_mailbox_get(&backend, &(), args, &[])
        .await
        .expect("Mailbox/get must succeed");
    assert_eq!(
        primary["list"],
        json!([{
            "id": "inbox",
            "totalEmails": 2, "unreadEmails": 1,
            "totalThreads": 1, "unreadThreads": 1,
        }])
    );
}

/// RFC 8621 §4.6: "the client must set each header field as an individual
/// property"; a `header:Date` or `header:Message-ID` property includes the
/// field, so the server generates neither. Expected fields are
/// hand-written RFC 5322.
#[tokio::test]
async fn create_writes_header_properties() {
    let backend = backend();
    let resp = set(
        &backend,
        json!({
            "mailboxIds": { "inbox": true },
            "header:Date:asDate": "2026-10-23T12:00:00Z",
            "header:Message-ID:asMessageIds": ["h1@example.com"],
            "header:X-Mailer:asText": "Mutt",
            "textBody": [{
                "partId": "1",
                "type": "text/plain",
                "header:X-Part": "p",
            }],
            "bodyValues": { "1": { "value": "Hi" } },
        }),
    )
    .await;
    let created = &resp["created"]["c0"];
    assert_eq!(
        stored_message(&backend, created["blobId"].as_str().unwrap()).await,
        "Date: 23 Oct 2026 12:00:00 +0000\r\n\
         Message-ID: <h1@example.com>\r\n\
         X-Mailer: Mutt\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         X-Part: p\r\n\
         Content-Transfer-Encoding: quoted-printable\r\n\
         \r\n\
         Hi"
    );
    let email = get(
        &backend,
        created["id"].as_str().unwrap(),
        &["header:X-Mailer:asText", "messageId"],
    )
    .await;
    assert_eq!(email["header:X-Mailer:asText"], json!("Mutt"));
    assert_eq!(email["messageId"], json!(["h1@example.com"]));
}

/// Each create breaks one RFC 8621 §4.6 constraint, which "SHOULD be
/// rejected with an "invalidProperties" error".
#[tokio::test]
async fn create_rejects_header_properties_breaking_rfc8621_4_6() {
    let text = json!([{ "partId": "1", "type": "text/plain" }]);
    let cases = [
        // "There MUST NOT be two properties that represent the same header
        // field (e.g., "header:from" and "from")".
        (
            json!({ "from": [{ "email": "a@example.com" }], "header:from": " b@example.com" }),
            Some("header:from"),
        ),
        // "Header fields beginning with "Content-" MUST NOT be specified on
        // the Email object".
        (
            json!({ "header:Content-Language": "en" }),
            Some("header:Content-Language"),
        ),
        // "The "headers" property MUST NOT be given".
        (
            json!({ "headers": [{ "name": "X-A", "value": " b" }] }),
            None,
        ),
        // "Header fields MUST NOT be specified in parsed forms that are
        // forbidden for that particular field."
        (json!({ "header:Date:asAddresses": [] }), None),
    ];
    for (extra, property) in cases {
        let backend = backend();
        let mut create = json!({
            "mailboxIds": { "inbox": true },
            "textBody": text,
            "bodyValues": { "1": { "value": "x" } },
        });
        create
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let resp = set(&backend, create.clone()).await;
        let err = &resp["notCreated"]["c0"];
        assert_eq!(err["type"], json!("invalidProperties"), "{create}");
        if let Some(property) = property {
            assert_eq!(err["properties"], json!([property]), "{create}");
        }
    }
}
