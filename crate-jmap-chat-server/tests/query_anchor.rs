//! `/query` honours `anchor` and `anchorOffset` (bd:JMAP-yt1w.22).
//!
//! Exercised through `Chat/query`, which delegates to the generic
//! `jmap_server::handle_query`, against the reference
//! `MemoryBackend`. With no `sort` the results are in id order, so five
//! Chats `c1`..`c5` sit at indexes 0..4.
//!
//! Oracle: RFC 8620 §5.5 (https://www.rfc-editor.org/rfc/rfc8620.txt,
//! Section 5.5 "/query").

mod common;

use common::{ACCOUNT_ID, MemoryBackend};
use jmap_chat_server::{handle_chat_query, handle_emoji_query};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

/// A backend holding Chats `c1`..`c5` in account [`ACCOUNT_ID`].
fn seeded_backend() -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    for id in ["c1", "c2", "c3", "c4", "c5"] {
        backend.insert_object_for_test("Chat", ACCOUNT_ID, id, json!({ "id": id }));
    }
    backend
}

/// Run `Chat/query` with `extra` merged into `{ "accountId": ACCOUNT_ID }`.
async fn query(backend: &MemoryBackend, extra: Value) -> Result<Value, Value> {
    let mut args = json!({ "accountId": ACCOUNT_ID });
    for (k, v) in extra.as_object().expect("extra must be an object") {
        args[k] = v.clone();
    }
    handle_chat_query(backend, &(), args)
        .await
        .map(|HandlerOutput { primary: resp, .. }| resp)
        .map_err(|e| serde_json::to_value(&e).expect("JmapError must serialize"))
}

/// `ids` of a `/query` response.
fn ids(resp: &Value) -> Vec<&str> {
    resp["ids"]
        .as_array()
        .expect("ids")
        .iter()
        .map(|id| id.as_str().expect("id"))
        .collect()
}

/// RFC 8620 §5.5: "If an "anchor" argument is given, the anchor is looked
/// for in the results after filtering and sorting.  If found, the
/// "anchorOffset" is then added to its index. ... This index is now used
/// exactly as though it were supplied as the "position" argument."
///
/// Anchor `c3` is index 2; offset +1 gives start index 3.
#[tokio::test]
async fn anchor_with_positive_offset_starts_after_anchor() {
    let backend = seeded_backend();
    let resp = query(
        &backend,
        json!({ "anchor": "c3", "anchorOffset": 1, "limit": 2 }),
    )
    .await
    .expect("Chat/query");
    assert_eq!(ids(&resp), ["c4", "c5"]);
    assert_eq!(resp["position"], 3);
}

/// RFC 8620 §5.5 anchorOffset: "This MAY be negative.  For example, "-1"
/// means the Foo immediately preceding the anchor is the first result in
/// the list returned".
#[tokio::test]
async fn anchor_with_negative_offset_starts_before_anchor() {
    let backend = seeded_backend();
    let resp = query(
        &backend,
        json!({ "anchor": "c3", "anchorOffset": -1, "limit": 2 }),
    )
    .await
    .expect("Chat/query");
    assert_eq!(ids(&resp), ["c2", "c3"]);
    assert_eq!(resp["position"], 1);
}

/// RFC 8620 §5.5 anchorOffset: "(default: 0)". Without an offset the
/// anchor itself is the first result.
#[tokio::test]
async fn anchor_without_offset_starts_at_anchor() {
    let backend = seeded_backend();
    let resp = query(&backend, json!({ "anchor": "c4" }))
        .await
        .expect("Chat/query");
    assert_eq!(ids(&resp), ["c4", "c5"]);
    assert_eq!(resp["position"], 3);
}

/// RFC 8620 §5.5: "If the resulting index is now negative, it is clamped
/// to 0."
///
/// Anchor `c2` is index 1; offset -3 gives -2, clamped to 0. The ignored
/// `position` keeps the expected page distinct from the `position` page.
#[tokio::test]
async fn anchor_offset_before_start_clamps_to_zero() {
    let backend = seeded_backend();
    let resp = query(
        &backend,
        json!({ "position": 3, "anchor": "c2", "anchorOffset": -3, "limit": 2 }),
    )
    .await
    .expect("Chat/query");
    assert_eq!(ids(&resp), ["c1", "c2"]);
    assert_eq!(resp["position"], 0);
}

/// RFC 8620 §5.5: "If the anchor is not found, the call is rejected with
/// an "anchorNotFound" error."
#[tokio::test]
async fn anchor_absent_from_results_is_anchor_not_found() {
    let backend = seeded_backend();
    let err = query(&backend, json!({ "anchor": "nope" }))
        .await
        .expect_err("an anchor absent from the results must be rejected");
    assert_eq!(err["type"], "anchorNotFound");
}

/// RFC 8620 §5.5: "If an "anchor" is specified, any position argument
/// supplied by the client MUST be ignored."
#[tokio::test]
async fn anchor_overrides_position() {
    let backend = seeded_backend();
    let resp = query(
        &backend,
        json!({ "position": 4, "anchor": "c2", "limit": 2 }),
    )
    .await
    .expect("Chat/query");
    assert_eq!(ids(&resp), ["c2", "c3"]);
    assert_eq!(resp["position"], 1);
}

/// RFC 8620 §5.5: "If no "anchor" is supplied, any "anchorOffset"
/// argument MUST be ignored."
///
/// Guards the fix against applying `anchorOffset` to `position`.
#[tokio::test]
async fn anchor_offset_without_anchor_is_ignored() {
    let backend = seeded_backend();
    let resp = query(
        &backend,
        json!({ "position": 1, "anchorOffset": 2, "limit": 2 }),
    )
    .await
    .expect("Chat/query");
    assert_eq!(ids(&resp), ["c2", "c3"]);
    assert_eq!(resp["position"], 1);
}

/// RFC 8620 §5.5 argument types: "anchor: "Id|null"" and
/// "anchorOffset: "Int" (default: 0)"; RFC 8620 §3.6.2 ""invalidArguments":
/// One of the arguments is of the wrong type or is otherwise invalid".
#[tokio::test]
async fn anchor_arguments_of_wrong_type_are_invalid_arguments() {
    let backend = seeded_backend();
    for extra in [
        json!({ "anchor": 7 }),
        json!({ "anchor": "c2", "anchorOffset": "1" }),
    ] {
        let err = query(&backend, extra.clone())
            .await
            .expect_err("wrongly typed anchor arguments must be rejected");
        assert_eq!(err["type"], "invalidArguments", "args: {extra}");
    }
}

/// RFC 8620 §5.5 types `limit` as "UnsignedInt|null" and `position` as
/// "Int"; wrong types are `invalidArguments` (§3.6.2). The description
/// names only the JSON type: §3.6.1 reserves it for "unstructured English
/// text suitable for displaying to a developer", so the client-supplied
/// value is never echoed (bd JMAP-zegc). `CustomEmoji/query` parses these
/// arguments itself rather than through `jmap_server::handle_query`.
#[tokio::test]
async fn emoji_query_wrongly_typed_args_do_not_echo_value() {
    let backend = seeded_backend();
    for name in ["limit", "position"] {
        let mut args = json!({ "accountId": ACCOUNT_ID });
        args[name] = json!({ "note": "CANARY-zegc" });
        let err = handle_emoji_query(&backend, &(), args)
            .await
            .expect_err("wrongly typed argument must be rejected");
        assert_eq!(err.error_type.to_string(), "invalidArguments", "{name}");
        let desc = err.description.as_deref().unwrap_or_default();
        assert!(desc.starts_with(name), "{desc}");
        assert!(desc.contains("got object"), "{desc}");
        assert!(!desc.contains("CANARY"), "{desc}");
    }
}
