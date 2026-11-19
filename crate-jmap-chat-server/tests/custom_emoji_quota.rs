//! `CustomEmoji/set` per-Space count and per-image size caps
//! (bd:JMAP-b9qs).
//!
//! Oracle: jmap-chat-implementer-guide.md §2.2 (L464-466): "Quotas: 100
//! emoji per Space, 256 KB per emoji image. Tune based on storage budget.
//! The server MUST reject `CustomEmoji/set` with `overQuota` (per
//! `{{RFC8620}}` §5.3) when the count or size limit is exceeded." The
//! atomic cases follow draft-ietf-jmap-conditional-02 §4.2 (an atomic
//! call is judged on its final state). Expected responses are
//! hand-written.

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use common::{ACCOUNT_ID, MemoryBackend, account_backend};
use jmap_chat_server::{
    AttachmentBlob, ChatLimits, ChatStore, handle_emoji_set, register_chat_conditional,
    register_chat_handlers,
};
use jmap_chat_types::CustomEmoji;
use jmap_server::{BackendSetError, Dispatcher, HandlerOutput, SetErrorType};
use jmap_types::{Id, JmapRequest, State, UTCDate};
use serde_json::{Value, json};

/// A backend whose emoji caps are `count` per Space and `bytes` per image.
fn capped(count: Option<u32>, bytes: Option<u64>) -> MemoryBackend {
    let backend = account_backend();
    backend.set_limits_for_test(Some(
        ChatLimits::default()
            .with_max_custom_emoji_per_space(count)
            .with_max_custom_emoji_bytes(bytes),
    ));
    backend
}

fn blob(backend: &MemoryBackend, blob_id: &str, size: u64) {
    backend.set_attachment_blob_for_test(
        ACCOUNT_ID,
        blob_id,
        Some(AttachmentBlob::new(size, [0; 32])),
    );
}

async fn set(backend: &MemoryBackend, body: Value) -> Value {
    let mut args = body;
    args["accountId"] = json!(ACCOUNT_ID);
    let HandlerOutput { primary, .. } = handle_emoji_set(backend, &(), &BTreeMap::new(), args)
        .await
        .expect("CustomEmoji/set");
    primary
}

fn emoji(name: &str, space: Option<&str>) -> Value {
    let mut e = json!({ "name": name, "blobId": "blob1" });
    if let Some(space) = space {
        e["spaceId"] = json!(space);
    }
    e
}

/// The guide's "Recommended starting point" is the default.
#[test]
fn defaults_are_the_guides_starting_point() {
    let limits = ChatLimits::default();
    assert_eq!(limits.max_custom_emoji_per_space, Some(100));
    assert_eq!(limits.max_custom_emoji_bytes, Some(256 * 1024));
}

/// "MUST reject ... with `overQuota` ... when the count ... limit is
/// exceeded": at a cap of 2 a Space takes two emoji; the third is
/// `overQuota`. Other Spaces and server-global emoji are not counted.
#[tokio::test]
async fn count_at_limit_then_limit_plus_one() {
    let backend = capped(Some(2), None);
    let resp = set(
        &backend,
        json!({ "create": { "a": emoji("a", Some("s1")), "b": emoji("b", Some("s1")) } }),
    )
    .await;
    assert_eq!(resp["notCreated"], Value::Null, "{resp}");
    assert!(resp["created"]["b"]["id"].is_string(), "{resp}");

    let resp = set(
        &backend,
        json!({ "create": {
            "c": emoji("c", Some("s1")),
            "other": emoji("c", Some("s2")),
            "global": emoji("c", None),
        } }),
    )
    .await;
    assert_eq!(resp["notCreated"]["c"]["type"], "overQuota", "{resp}");
    assert!(resp["created"]["other"]["id"].is_string(), "{resp}");
    assert!(resp["created"]["global"]["id"].is_string(), "{resp}");
}

/// "MUST reject ... with `overQuota` ... when the ... size limit is
/// exceeded": at a cap of 1000 bytes a 1000-byte image is accepted and a
/// 1001-byte one is `overQuota`, on create and on a `blobId` update.
#[tokio::test]
async fn size_at_limit_then_limit_plus_one() {
    let backend = capped(None, Some(1000));
    blob(&backend, "fits", 1000);
    blob(&backend, "big", 1001);
    let resp = set(
        &backend,
        json!({ "create": {
            "ok": { "name": "ok", "blobId": "fits" },
            "no": { "name": "no", "blobId": "big" },
        } }),
    )
    .await;
    assert_eq!(resp["notCreated"]["no"]["type"], "overQuota", "{resp}");
    let id = resp["created"]["ok"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("created: {resp}"))
        .to_owned();

    let resp = set(&backend, json!({ "update": { &id: { "blobId": "big" } } })).await;
    assert_eq!(resp["notUpdated"][&id]["type"], "overQuota", "{resp}");
    let resp = set(&backend, json!({ "update": { &id: { "blobId": "fits" } } })).await;
    assert_eq!(resp["notUpdated"], Value::Null, "{resp}");
}

/// `None` is no limit.
#[tokio::test]
async fn none_is_no_limit() {
    let backend = capped(None, None);
    blob(&backend, "huge", u64::MAX);
    let create: serde_json::Map<String, Value> = (0..101)
        .map(|n| (format!("e{n}"), emoji(&format!("e{n}"), Some("s1"))))
        .chain([("h".to_owned(), json!({ "name": "h", "blobId": "huge" }))])
        .collect();
    let resp = set(&backend, json!({ "create": create })).await;
    assert_eq!(resp["notCreated"], Value::Null, "{resp}");
}

fn built(name: &'static str, blob_id: &'static str) -> impl FnOnce(&Id) -> CustomEmoji + Send {
    move |id: &Id| {
        let mut e = CustomEmoji::new(
            id.clone(),
            name,
            Id::from(blob_id),
            Id::from(ACCOUNT_ID),
            UTCDate::from("2026-01-01T00:00:00Z"),
        );
        e.space_id = Some(Id::from("s1"));
        e
    }
}

fn over_quota<T: std::fmt::Debug>(r: Result<T, BackendSetError<common::MemoryError>>) -> bool {
    matches!(r, Err(BackendSetError::SetError(e)) if e.error_type == SetErrorType::OverQuota)
}

/// The backend enforces both caps itself, for a caller that skips the
/// handler's pre-check.
#[tokio::test]
async fn backend_enforces_both_caps() {
    let backend = capped(Some(1), Some(1000));
    blob(&backend, "big", 1001);
    let acc = Id::from(ACCOUNT_ID);
    backend
        .create_object(&(), &acc, "a", built("a", "blob1"))
        .await
        .expect("first emoji fits");
    assert!(over_quota(
        backend
            .create_object(&(), &acc, "b", built("b", "blob1"))
            .await
    ));

    let backend = capped(None, Some(1000));
    blob(&backend, "big", 1001);
    assert!(over_quota(
        backend
            .create_object(&(), &acc, "c", built("c", "big"))
            .await
    ));
}

// ---------------------------------------------------------------------------
// Atomic conditional /set: the count is judged on the final state.
// ---------------------------------------------------------------------------

const USING: &[&str] = &[
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:chat",
    "urn:ietf:params:jmap:conditional",
];

fn dispatcher(b: &Arc<MemoryBackend>) -> Dispatcher<()> {
    let mut d = Dispatcher::new();
    register_chat_handlers(&mut d, Arc::clone(b));
    register_chat_conditional(&mut d, Arc::clone(b));
    d
}

async fn call(d: &Dispatcher<()>, mut args: Value) -> (String, Value) {
    args["accountId"] = json!(ACCOUNT_ID);
    let req: JmapRequest = serde_json::from_value(
        json!({"using": USING, "methodCalls": [["CustomEmoji/set", args, "0"]]}),
    )
    .expect("request");
    let resp = serde_json::to_value(d.dispatch(req, (), State::from("s")).await).expect("json");
    let inv = &resp["methodResponses"][0];
    (inv[0].as_str().expect("name").to_owned(), inv[1].clone())
}

/// A Space at its cap of 1 with emoji `e1`.
fn full_space() -> Arc<MemoryBackend> {
    let b = capped(Some(1), None);
    b.insert_object_for_test(
        "CustomEmoji",
        ACCOUNT_ID,
        "e1",
        json!({"id": "e1", "name": "old", "blobId": "blob1", "spaceId": "s1",
               "createdBy": ACCOUNT_ID, "createdAt": "2026-01-01T00:00:00Z"}),
    );
    Arc::new(b)
}

/// Creates run before destroys, so replacing an emoji in a full Space
/// fails without `atomic` and succeeds with it: the final state holds one.
#[tokio::test]
async fn atomic_replace_in_a_full_space() {
    let replace = |atomic: bool| json!({ "atomic": atomic, "create": { "n": emoji("new", Some("s1")) }, "destroy": ["e1"] });
    let d = dispatcher(&full_space());
    let (_, r) = call(&d, replace(false)).await;
    assert_eq!(r["notCreated"]["n"]["type"], "overQuota", "{r}");
    assert_eq!(r["destroyed"], json!(["e1"]), "{r}");

    let d = dispatcher(&full_space());
    let (name, r) = call(&d, replace(true)).await;
    assert_eq!(name, "CustomEmoji/set", "{r}");
    assert!(r["created"]["n"]["id"].is_string(), "{r}");
    assert_eq!(r["destroyed"], json!(["e1"]), "{r}");
}

/// An atomic create past the cap fails the whole call.
#[tokio::test]
async fn atomic_create_past_the_cap_is_atomic_failure() {
    let b = capped(Some(1), None);
    let b = Arc::new(b);
    let d = dispatcher(&b);
    let (name, r) = call(
        &d,
        json!({ "atomic": true, "create": {
            "a": emoji("a", Some("s1")), "b": emoji("b", Some("s1")),
        } }),
    )
    .await;
    assert_eq!(name, "error", "{r}");
    assert_eq!(r["type"], "atomicFailure", "{r}");
    let not_created = r["notCreated"].as_object().expect("notCreated");
    assert!(
        not_created.values().all(|e| e["type"] == "overQuota"),
        "{r}"
    );
}
