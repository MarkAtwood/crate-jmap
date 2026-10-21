//! `VTCCall/set` create (draft-atwood-jmap-vtc-00 §Creating a Ring Call,
//! §Creating a Room Call, §Creating a Scheduled Call, L573). Each assertion
//! quotes its oracle; line numbers cite draft-atwood-jmap-vtc-00.md at
//! jmap-chat-spec 3227c0f.

mod common;

use common::*;
use jmap_server::JmapBackend;
use jmap_types::Id;
use jmap_vtc_types::{CallResponse, CallRole, VtcParticipant};
use serde_json::{json, Value};

/// The ring create example, L594-604, verbatim.
fn ring_example() -> Value {
    json!({
        "callType": "ring",
        "targetParticipantIds": ["user:alice@example.com"],
        "mediaTypes": ["audio", "video"],
        "subject": "Quick sync"
    })
}

fn room_call() -> Value {
    json!({ "callType": "room", "mediaTypes": ["audio"] })
}

fn scheduled_call(start: &str) -> Value {
    json!({ "callType": "scheduled", "mediaTypes": ["audio"], "scheduledStartAt": start })
}

async fn participants(backend: &jmap_vtc_server::memory::MemoryBackend) -> Vec<VtcParticipant> {
    backend
        .get_objects::<VtcParticipant>(&BOB.to_owned(), &Id::from(ACCOUNT), None, None)
        .await
        .expect("participants")
        .0
}

fn assert_set_error(err: &Value, kind: &str, property: Option<&str>) {
    assert_eq!(err["type"], kind, "{err}");
    if let Some(p) = property {
        assert_eq!(err["properties"], json!([p]), "{err}");
    }
}

#[tokio::test]
async fn ring_example_creates_a_ringing_call() {
    let backend = backend();
    let created = create(&backend, BOB, ring_example()).await;

    // jmap-chat-spec 6bcd9d8 L662-671: the example `created` entry has
    // `"state": "ringing"`, `"initiatorId": "user:bob@example.com"`,
    // `"activeParticipantCount": 1`;
    // L434 "`"creating"` → `"ringing"`: Server transitions automatically
    // when the call is created and ring notifications have been dispatched."
    assert_eq!(created["state"], "ringing");
    assert_eq!(created["initiatorId"], BOB);
    assert_eq!(created["activeParticipantCount"], 1);
    // L589 "The server sets `id`, `initiatorId`, `createdAt`, `state`, and
    // `joinUri`."
    assert!(created["createdAt"].is_string());
    assert!(!created["joinUri"].as_str().unwrap_or_default().is_empty());
    // L338 "VTCCall IDs are ULIDs": 26 characters of Crockford base32
    // (ULID spec, github.com/ulid/spec).
    let id = created["id"].as_str().expect("id");
    assert_eq!(id.len(), 26, "{id}");
    assert!(
        id.chars()
            .all(|c| "0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(c)),
        "{id}"
    );
}

#[tokio::test]
async fn ring_create_makes_participant_records_for_targets_and_initiator() {
    let backend = backend();
    let created = create(&backend, BOB, ring_example()).await;
    let call_id = created["id"].as_str().expect("id");

    // jmap-chat-spec 6bcd9d8 L633 "The server creates a VTCParticipant
    // record for each target (with `joinedAt: null` and `callResponse:
    // "pending"`) and one for the initiator (with `joinedAt` set to the
    // creation time, since the initiator is in the call from the start)".
    let parts = participants(&backend).await;
    assert_eq!(parts.len(), 2, "{parts:?}");
    let by_user = |u: &str| {
        parts
            .iter()
            .find(|p| p.user_id.as_deref() == Some(u))
            .unwrap_or_else(|| panic!("no record for {u}: {parts:?}"))
    };
    let alice = by_user(ALICE);
    let bob = by_user(BOB);
    for p in [alice, bob] {
        assert_eq!(p.call_id.as_ref(), call_id);
    }
    assert_eq!(alice.joined_at, None);
    assert_eq!(
        bob.joined_at.as_ref().map(|t| json!(t)),
        Some(created["createdAt"].clone())
    );
    // L294 "The call initiator receives `"moderator"` by default".
    assert_eq!(bob.role, CallRole::Moderator);
    // L303 "The server sets this to `"pending"` when creating VTCParticipant
    // records for ring targets".
    assert_eq!(alice.call_response, Some(CallResponse::Pending));
}

#[tokio::test]
async fn room_example_creates_an_active_call() {
    let backend = backend();
    backend.advertise(&Id::from(ACCOUNT), CHAT_URI);
    // L639-649 verbatim.
    let created = create(
        &backend,
        BOB,
        json!({
            "callType": "room",
            "mediaTypes": ["audio"],
            "subject": "Team Huddle",
            "spaceId": "01J3ABCDEF0000000000000000",
            "channelId": "01J3ABCDEF1111111111111111"
        }),
    )
    .await;
    // L634 "The server sets `id`, `initiatorId`, `createdAt`, `state` (to
    // `"active"`), and `joinUri`."; L443 "Room calls transition directly to
    // `"active"` on creation."
    assert_eq!(created["state"], "active");
    assert_eq!(created["initiatorId"], BOB);
    assert_eq!(created["spaceId"], "01J3ABCDEF0000000000000000");
    // Room calls have no ring targets, so no VTCParticipant records yet.
    assert!(participants(&backend).await.is_empty());
}

#[tokio::test]
async fn scheduled_example_creates_a_pending_call() {
    let backend = backend();
    backend.advertise(&Id::from(ACCOUNT), CHAT_URI);
    // L671-682, with `scheduledStartAt` moved from 2026-06-06 to 2099 so it
    // is still "in the future" (L659) when the test runs.
    let created = create(
        &backend,
        BOB,
        json!({
            "callType": "scheduled",
            "mediaTypes": ["audio", "video"],
            "scheduledStartAt": "2099-06-06T10:00:00Z",
            "subject": "Sprint Planning",
            "lobbyEnabled": true,
            "spaceId": "01J3ABCDEF0000000000000000"
        }),
    )
    .await;
    // L666 "The server sets ... `state` (to `"pending"`)".
    assert_eq!(created["state"], "pending");
    assert_eq!(created["scheduledStartAt"], "2099-06-06T10:00:00Z");
    assert_eq!(created["lobbyEnabled"], true);
}

#[tokio::test]
async fn scheduled_start_in_the_past_is_invalid() {
    let backend = backend();
    // L685 "Servers MUST return `invalidProperties` naming `scheduledStartAt`
    // when `scheduledStartAt` is in the past." The draft's own example date,
    // 2026-06-06, is now past.
    let err = create_error(&backend, BOB, scheduled_call("2026-06-06T10:00:00Z")).await;
    assert_set_error(&err, "invalidProperties", Some("scheduledStartAt"));
    // L658 "`scheduledStartAt` (UTCDate, required)".
    let err = create_error(
        &backend,
        BOB,
        json!({ "callType": "scheduled", "mediaTypes": ["audio"] }),
    )
    .await;
    assert_set_error(&err, "invalidProperties", Some("scheduledStartAt"));
}

#[tokio::test]
async fn ring_without_targets_is_invalid() {
    let backend = backend();
    // L623 "Servers MUST return `invalidProperties` naming
    // `targetParticipantIds` when `targetParticipantIds` is empty."
    let mut call = ring_example();
    call["targetParticipantIds"] = json!([]);
    let err = create_error(&backend, BOB, call).await;
    assert_set_error(&err, "invalidProperties", Some("targetParticipantIds"));
    // L579 "`targetParticipantIds` (String[], required)".
    let mut call = ring_example();
    call.as_object_mut()
        .map(|m| m.remove("targetParticipantIds"));
    let err = create_error(&backend, BOB, call).await;
    assert_set_error(&err, "invalidProperties", Some("targetParticipantIds"));
}

#[tokio::test]
async fn unsupported_media_type_is_invalid() {
    let backend = backend();
    // L623 "Servers MUST return `invalidProperties` naming `mediaTypes` when
    // `mediaTypes` contains values not present in the account-level
    // `supportedMediaTypes`." The fixture supports audio and video only.
    let mut call = ring_example();
    call["mediaTypes"] = json!(["audio", "screen"]);
    let err = create_error(&backend, BOB, call).await;
    assert_set_error(&err, "invalidProperties", Some("mediaTypes"));
    // L382 `mediaTypes`: "Subset of `supportedMediaTypes`", for every type.
    let err = create_error(
        &backend,
        BOB,
        json!({ "callType": "room", "mediaTypes": ["screen"] }),
    )
    .await;
    assert_set_error(&err, "invalidProperties", Some("mediaTypes"));
}

#[tokio::test]
async fn may_create_call_false_is_forbidden_for_every_type() {
    let mut cap = capability();
    cap["mayCreateCall"] = json!(false);
    let backend = backend_with(cap);
    // L623 ring, L652 room, L685 scheduled: "Servers MUST return `forbidden`
    // when `mayCreateCall` is `false`".
    for call in [
        ring_example(),
        room_call(),
        scheduled_call("2099-01-01T00:00:00Z"),
    ] {
        let err = create_error(&backend, BOB, call).await;
        assert_set_error(&err, "forbidden", None);
    }
}

#[tokio::test]
async fn unsupported_call_type_is_forbidden() {
    // L623 "Servers MUST return `forbidden` when `supportsRingCalls` is
    // `false`."; L652 "... or `supportsRoomCalls` is `false`."; L685 "... or
    // `supportsScheduledCalls` is `false`."
    for (flag, call) in [
        ("supportsRingCalls", ring_example()),
        ("supportsRoomCalls", room_call()),
        (
            "supportsScheduledCalls",
            scheduled_call("2099-01-01T00:00:00Z"),
        ),
    ] {
        let mut cap = capability();
        cap[flag] = json!(false);
        let backend = backend_with(cap);
        let err = create_error(&backend, BOB, call).await;
        assert_set_error(&err, "forbidden", None);
    }
}

#[tokio::test]
async fn chat_id_without_chat_capability_is_invalid() {
    let backend = backend();
    // L623 / L652 / L685 "When `chatId` is supplied but
    // `urn:ietf:params:jmap:chat` is not present in the server's
    // capabilities, the server MUST return `invalidProperties` naming
    // `chatId` with a description indicating the Chat capability is
    // required."
    for mut call in [
        ring_example(),
        room_call(),
        scheduled_call("2099-01-01T00:00:00Z"),
    ] {
        call["chatId"] = json!("C1");
        let err = create_error(&backend, BOB, call).await;
        assert_set_error(&err, "invalidProperties", Some("chatId"));
        let description = err["description"].as_str().unwrap_or_default();
        assert!(description.contains(CHAT_URI), "{err}");
    }
}

#[tokio::test]
async fn second_active_call_on_a_chat_is_invalid() {
    let backend = backend();
    backend.advertise(&Id::from(ACCOUNT), CHAT_URI);
    let mut first = room_call();
    first["chatId"] = json!("C1");
    let first = create(&backend, BOB, first).await;

    // L573 "When creating a VTCCall with a `chatId` that is already
    // referenced by another VTCCall in `"active"` or `"ringing"` state, the
    // server MUST return an `invalidProperties` SetError naming `chatId`,
    // with description "A call is already active on this Chat.""
    let mut second = ring_example();
    second["chatId"] = json!("C1");
    let err = create_error(&backend, ALICE, second.clone()).await;
    assert_eq!(
        err,
        json!({
            "type": "invalidProperties",
            "properties": ["chatId"],
            "description": "A call is already active on this Chat."
        })
    );

    // Another Chat is unaffected.
    let mut other = room_call();
    other["chatId"] = json!("C2");
    create(&backend, ALICE, other).await;

    // Once the first call has ended it no longer blocks the Chat.
    let id = first["id"].as_str().expect("id");
    update(&backend, BOB, id, json!({ "state": "ended" }))
        .await
        .expect("end");
    create(&backend, ALICE, second).await;
}

#[tokio::test]
async fn ringing_call_also_blocks_its_chat() {
    let backend = backend();
    backend.advertise(&Id::from(ACCOUNT), CHAT_URI);
    let mut ring = ring_example();
    ring["chatId"] = json!("C1");
    assert_eq!(create(&backend, BOB, ring).await["state"], "ringing");
    // L573 "in `"active"` or `"ringing"` state".
    let mut room = room_call();
    room["chatId"] = json!("C1");
    let err = create_error(&backend, ALICE, room).await;
    assert_set_error(&err, "invalidProperties", Some("chatId"));
}

#[tokio::test]
async fn server_set_properties_are_refused_on_create() {
    let backend = backend();
    // L589 "The server sets `id`, `initiatorId`, `createdAt`, `state`, and
    // `joinUri`."
    for (key, value) in [
        ("id", json!("X")),
        ("initiatorId", json!(EVE)),
        ("createdAt", json!("2026-01-01T00:00:00Z")),
        ("state", json!("active")),
        ("joinUri", json!("https://evil.example/")),
    ] {
        let mut call = ring_example();
        call[key] = value;
        let err = create_error(&backend, BOB, call).await;
        assert_set_error(&err, "invalidProperties", Some(key));
    }
}

#[tokio::test]
async fn unknown_call_type_is_invalid() {
    let backend = backend();
    // L349 `callType`: "`"ring"`, `"room"`, or `"scheduled"`".
    let err = create_error(
        &backend,
        BOB,
        json!({ "callType": "conference", "mediaTypes": ["audio"] }),
    )
    .await;
    assert_set_error(&err, "invalidProperties", Some("callType"));
}
