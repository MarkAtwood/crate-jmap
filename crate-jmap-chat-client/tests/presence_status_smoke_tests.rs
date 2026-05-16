//! Wiremock smoke tests for `PresenceStatus/*` method paths in
//! jmap-chat-client.
//!
//! Spec oracles:
//!   - RFC 8620 §5.1 /get, §5.2 /changes, §5.3 /set
//!   - draft-atwood-jmap-chat-00 §4.21 (PresenceStatus object field set)
//!     and §5 (PresenceStatus/* method-specific shapes)

#[path = "helpers.rs"]
mod helpers;

use helpers::{
    jmap_response, mock_jmap_post, recorded_args, recorded_body, set_response, TEST_ACCOUNT_ID,
};
use jmap_types::{Id, State, UTCDate};
use serde_json::json;
use wiremock::MockServer;

/// `PresenceStatus/get` is singleton-shaped: caller passes no ids, and
/// the wire request MUST emit `"ids": null` so the server returns the
/// single PresenceStatus record for the account (draft-atwood-jmap-chat-00
/// §4.21). Pins the USING_CHAT capability set for the entire
/// PresenceStatus/* family (one assertion per method-family per
/// workspace convention).
#[tokio::test]
async fn presence_status_get_sends_ids_null_singleton_shape() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "PresenceStatus/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": "ps-state-1",
            "list": [
                {
                    "id": "ps-1",
                    "presence": "online",
                    "receiptSharing": true,
                    "updatedAt": "2026-01-01T00:00:00Z"
                }
            ],
            "notFound": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let resp = sc
        .presence_status_get()
        .await
        .expect("presence_status_get: must succeed");

    assert_eq!(resp.list.len(), 1, "list must contain the singleton");
    let ps = &resp.list[0];
    assert_eq!(ps.id.as_ref(), "ps-1", "id mismatch");
    assert!(
        matches!(ps.presence, jmap_chat_types::Presence::Online),
        "presence mismatch"
    );
    assert!(ps.receipt_sharing, "receipt_sharing must be true");

    let body = recorded_body(&server).await;
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["accountId"],
        json!(TEST_ACCOUNT_ID),
        "accountId mismatch"
    );
    // Singleton shape: ids MUST be present as JSON null (not absent).
    // Absence would let the server interpret it as "all" instead of
    // "the singleton", which is identical for a one-element type but
    // is a spec-defined contract difference.
    assert!(
        args.get("ids").is_some(),
        "ids key must be present (singleton shape)"
    );
    assert_eq!(args["ids"], json!(null), "ids must be JSON null");
    // RFC 8620 §3.3 — PresenceStatus/* MUST declare USING_CHAT.
    assert_eq!(
        body["using"],
        json!(["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:chat"]),
        "PresenceStatus/* using must equal USING_CHAT exactly"
    );
}

/// `PresenceStatus/get` decode coverage: populated wire object with
/// every optional field (`status_text`, `status_emoji`, `expires_at`)
/// must round-trip through the [`jmap_chat_types::PresenceStatus`]
/// `Deserialize` impl.
#[tokio::test]
async fn presence_status_get_decodes_populated_record() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "PresenceStatus/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": "ps-state-2",
            "list": [
                {
                    "id": "ps-1",
                    "presence": "away",
                    "receiptSharing": false,
                    "updatedAt": "2026-01-20T12:00:00Z",
                    "statusText": "In a meeting",
                    "statusEmoji": "🤝",
                    "expiresAt": "2026-01-20T13:00:00Z"
                }
            ],
            "notFound": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let resp = sc
        .presence_status_get()
        .await
        .expect("presence_status_get: must succeed");

    let ps = &resp.list[0];
    assert!(
        matches!(ps.presence, jmap_chat_types::Presence::Away),
        "presence must deserialise to Away"
    );
    assert!(!ps.receipt_sharing, "receipt_sharing must be false");
    assert_eq!(
        ps.status_text.as_deref(),
        Some("In a meeting"),
        "status_text mismatch"
    );
    assert_eq!(
        ps.status_emoji.as_deref(),
        Some("🤝"),
        "status_emoji mismatch"
    );
    assert_eq!(
        ps.expires_at.as_ref().map(|d| d.as_ref()),
        Some("2026-01-20T13:00:00Z"),
        "expires_at mismatch"
    );
}

/// `PresenceStatus/set` update with `presence` + `status_text` +
/// `receipt_sharing` must thread all three through the patch object
/// using camelCase wire keys (misc.rs:116-145, RFC 8620 §5.3).
/// `status_emoji` and `expires_at` left at `Patch::Keep` (the default)
/// must be absent from the patch.
#[tokio::test]
async fn presence_status_update_patch_serialises_fields() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "PresenceStatus/set",
        "ps-1",
        "ps-2",
        json!({ "updated": { "ps-1": null } }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let id = Id::from("ps-1");
    let mut patch = jmap_chat_client::methods::PresenceStatusPatch::default();
    patch.presence = Some(jmap_chat_types::Presence::Busy);
    patch.status_text = jmap_chat_client::methods::Patch::Set("Heads down");
    patch.receipt_sharing = Some(true);
    let _ = sc
        .presence_status_update(&id, &patch)
        .await
        .expect("presence_status_update: must succeed");

    let args = recorded_args(&server).await;
    let patch_obj = &args["update"]["ps-1"];
    assert_eq!(
        patch_obj["presence"],
        json!("busy"),
        "presence must serialise as 'busy'"
    );
    assert_eq!(
        patch_obj["statusText"],
        json!("Heads down"),
        "statusText mismatch"
    );
    assert_eq!(
        patch_obj["receiptSharing"],
        json!(true),
        "receiptSharing mismatch"
    );
    assert!(
        patch_obj.get("statusEmoji").is_none(),
        "statusEmoji must be absent when Patch::Keep (default)"
    );
    assert!(
        patch_obj.get("expiresAt").is_none(),
        "expiresAt must be absent when Patch::Keep (default)"
    );
}

/// `PresenceStatus/set` update with `expires_at: Patch::Clear` must emit
/// `"expiresAt": null` to clear the server-side deadline (RFC 8620 §5.3
/// patch null-clear semantics). Pairs with the implicit `Patch::Set` /
/// `Patch::Keep` cases exercised by the patch-fields test above.
#[tokio::test]
async fn presence_status_update_expires_at_clear_emits_null() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "PresenceStatus/set",
        "ps-1",
        "ps-2",
        json!({ "updated": { "ps-1": null } }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let id = Id::from("ps-1");
    let mut patch = jmap_chat_client::methods::PresenceStatusPatch::default();
    patch.expires_at = jmap_chat_client::methods::Patch::Clear;
    let _ = sc
        .presence_status_update(&id, &patch)
        .await
        .expect("presence_status_update: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["update"]["ps-1"],
        json!({ "expiresAt": null }),
        "Patch::Clear must serialise expiresAt as JSON null"
    );
}

/// `PresenceStatus/set` update with `expires_at: Patch::Set(_)` must
/// serialise the UTCDate verbatim. Confirms the Patch<&UTCDate>
/// boundary delegates to the underlying `UTCDate` serialize without
/// modification.
#[tokio::test]
async fn presence_status_update_expires_at_set_emits_value() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "PresenceStatus/set",
        "ps-1",
        "ps-2",
        json!({ "updated": { "ps-1": null } }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let id = Id::from("ps-1");
    let expires = UTCDate::from("2026-01-20T13:00:00Z");
    let mut patch = jmap_chat_client::methods::PresenceStatusPatch::default();
    patch.expires_at = jmap_chat_client::methods::Patch::Set(&expires);
    let _ = sc
        .presence_status_update(&id, &patch)
        .await
        .expect("presence_status_update: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["update"]["ps-1"],
        json!({ "expiresAt": "2026-01-20T13:00:00Z" }),
        "Patch::Set(&UTCDate) must serialise the wire string verbatim"
    );
}

/// `PresenceStatus/changes` must thread `since_state` and `max_changes`
/// and reject empty `since_state` client-side (misc.rs:167-171,
/// RFC 8620 §5.2).
#[tokio::test]
async fn presence_status_changes_passthrough_and_empty_state_rejected() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "PresenceStatus/changes",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "oldState": "ps-old",
            "newState": "ps-new",
            "hasMoreChanges": false,
            "created": [],
            "updated": ["ps-1"],
            "destroyed": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let since = State::from("ps-old");
    let _ = sc
        .presence_status_changes(&since, Some(10))
        .await
        .expect("presence_status_changes: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(args["sinceState"], json!("ps-old"), "sinceState mismatch");
    assert_eq!(args["maxChanges"], json!(10), "maxChanges mismatch");

    // Empty-state guard.
    let empty = State::from("");
    let err = sc
        .presence_status_changes(&empty, None)
        .await
        .expect_err("presence_status_changes must reject empty since_state");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(
                msg.contains("since_state may not be empty"),
                "error message must explain validation: got {msg:?}"
            );
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}
