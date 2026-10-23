//! `supportedBodyTypes` is configurable per account and governs
//! `Message/set` create (bd:JMAP-405z.5).
//!
//! A consumer declares its body types through [`ChatLimits`]: the
//! advertised list and the subset a companion specification reserves for
//! server-created Messages (e.g. `application/mimi-fanout`).

mod common;

use common::{ACCOUNT_ID, MemoryBackend, seed_group_chat};
use jmap_chat_server::ChatLimits;
use jmap_chat_server::message::handle_message_set;
use jmap_server::HandlerOutput;
use serde_json::{Value, json};

/// Send one `Message/set` create per `(createId, bodyType)` pair and return
/// the whole response. A `None` bodyType omits the property.
async fn create(backend: &MemoryBackend, entries: &[(&str, Option<&str>)]) -> Value {
    seed_group_chat(backend, "c1");
    let create: serde_json::Map<String, Value> = entries
        .iter()
        .map(|&(cid, body_type)| {
            let mut obj = json!({ "chatId": "c1", "body": "hi", "sentAt": "2026-01-01T00:00:00Z" });
            if let Some(t) = body_type {
                obj["bodyType"] = json!(t);
            }
            (cid.to_owned(), obj)
        })
        .collect();
    let HandlerOutput { primary: resp, .. } = handle_message_set(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "create": create }),
    )
    .await
    .expect("Message/set");
    resp
}

fn invalid_body_type() -> Value {
    json!({ "type": "invalidProperties", "properties": ["bodyType"] })
}

/// Oracle: draft-atwood-jmap-chat-00 §Account-Level Capability Object
/// `supportedBodyTypes`: "The server MUST accept in `Message/set` create
/// every listed type except types a companion specification reserves for
/// server-created Messages."
///
/// A type the deployment lists (here `application/mimi-content`, which
/// "Servers participating in MIMI federation domains MAY also include") must
/// be accepted on create.
#[tokio::test]
async fn configured_listed_body_type_is_accepted_on_create() {
    const TYPES: &[&str] = &["text/plain", "application/mimi-content"];
    let backend = MemoryBackend::new();
    backend.set_limits_for_test(Some(ChatLimits::default().with_supported_body_types(TYPES)));

    let resp = create(&backend, &[("m", Some("application/mimi-content"))]).await;
    assert!(
        resp["created"]["m"]["id"].is_string(),
        "listed bodyType must be accepted on create: {resp}"
    );
}

/// Oracle: draft-atwood-jmap-chat-00 §Account-Level Capability Object
/// `supportedBodyTypes`: "The server MUST accept in `Message/set` create
/// every listed type except types a companion specification reserves for
/// server-created Messages."; §Input Validation: "`bodyType`: reject a value
/// not listed in `supportedBodyTypes` ({{capability}}); in `Message/set`
/// create, also reject a type that a companion specification reserves for
/// server-created Messages."
///
/// The SetError type is left to the reserving companion specification
/// (draft-atwood-jmap-chat-mimi-00 §application/mimi-fanout: "the server
/// MUST reject a `Message/set` create with this body type with an
/// `invalidProperties` SetError naming `bodyType`"); the server applies that
/// SetError to every reserved type, so this test uses a placeholder type
/// and checks it while a non-reserved listed type in the same request is
/// accepted.
#[tokio::test]
async fn reserved_body_type_is_listed_but_rejected_on_create() {
    const RESERVED_TYPE: &str = "application/x-example-server-only";
    const TYPES: &[&str] = &["text/plain", RESERVED_TYPE];
    const RESERVED: &[&str] = &[RESERVED_TYPE];
    let backend = MemoryBackend::new();
    backend.set_limits_for_test(Some(
        ChatLimits::default()
            .with_supported_body_types(TYPES)
            .with_server_reserved_body_types(RESERVED),
    ));

    let resp = create(
        &backend,
        &[
            ("reserved", Some(RESERVED_TYPE)),
            ("plain", Some("text/plain")),
        ],
    )
    .await;
    assert!(
        resp["created"]["reserved"].is_null(),
        "reserved bodyType must be rejected on create: {resp}"
    );
    assert_eq!(
        resp["notCreated"]["reserved"]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(
        resp["notCreated"]["reserved"]["properties"],
        json!(["bodyType"]),
        "{resp}"
    );
    assert!(
        resp["created"]["plain"]["id"].is_string(),
        "non-reserved listed bodyType must be accepted: {resp}"
    );
}

/// Oracle: draft-atwood-jmap-chat-00 §Account-Level Capability Object
/// `supportedBodyTypes`: "MUST include `"text/plain"` unless the server
/// accepts only end-to-end encrypted body types ({{e2ee-fields}})."; §Input
/// Validation: "`bodyType`: reject a value not listed in
/// `supportedBodyTypes` ({{capability}})".
///
/// An E2EE-only deployment lists only `application/mls-ciphertext`; it must
/// accept that type and reject `text/plain`, whether explicit or implied by
/// an omitted `bodyType`.
#[tokio::test]
async fn e2ee_only_list_rejects_text_plain() {
    const TYPES: &[&str] = &["application/mls-ciphertext"];
    let backend = MemoryBackend::new();
    backend.set_limits_for_test(Some(ChatLimits::default().with_supported_body_types(TYPES)));

    let resp = create(
        &backend,
        &[
            ("cipher", Some("application/mls-ciphertext")),
            ("plain", Some("text/plain")),
            ("omitted", None),
        ],
    )
    .await;
    assert!(
        resp["created"]["cipher"]["id"].is_string(),
        "listed encrypted bodyType must be accepted: {resp}"
    );
    assert_eq!(resp["notCreated"]["plain"], invalid_body_type());
    assert_eq!(resp["notCreated"]["omitted"], invalid_body_type());
}

/// Oracle: draft-atwood-jmap-chat-00 §Account-Level Capability Object
/// `supportedBodyTypes`: "MUST include `"text/plain"` unless the server
/// accepts only end-to-end encrypted body types ({{e2ee-fields}})." and
/// "The server MUST accept in `Message/set` create every listed type except
/// types a companion specification reserves for server-created Messages."
///
/// The default (non-E2EE-only) configuration lists `text/plain`, reserves
/// nothing, and accepts every type it lists.
#[tokio::test]
async fn default_body_types_include_text_plain_and_are_all_accepted() {
    let limits = ChatLimits::default();
    assert!(limits.supported_body_types.contains(&"text/plain"));
    assert!(limits.server_reserved_body_types.is_empty());

    let backend = MemoryBackend::new();
    let entries: Vec<(String, Option<&str>)> = limits
        .supported_body_types
        .iter()
        .enumerate()
        .map(|(i, &t)| (format!("m{i}"), Some(t)))
        .collect();
    let entries: Vec<(&str, Option<&str>)> =
        entries.iter().map(|(cid, t)| (cid.as_str(), *t)).collect();
    let resp = create(&backend, &entries).await;
    assert_eq!(resp["notCreated"], json!(null), "{resp}");
}
