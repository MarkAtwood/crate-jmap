//! Restrictions on a Chat with a `roomId`, against
//! draft-atwood-jmap-chat-mimi-00 (jmap-chat-spec 6bcd9d8). Chats are
//! hand-written `extra` maps; each test quotes the draft line it checks.

use jmap_chat_mimi_server::SetError;
use jmap_chat_mimi_server::{
    may_create_room_message, may_update_room_chat, may_update_room_message,
};
use serde_json::{json, Map, Value};

fn extra(value: Value) -> Map<String, Value> {
    value.as_object().expect("object").clone()
}

fn bound() -> Map<String, Value> {
    extra(json!({ "roomId": "mimi://a.example/r/k5qwmzlqmfsgk3tbnvsxi2lf4a" }))
}

fn error(result: Result<(), SetError>) -> Value {
    serde_json::to_value(result.expect_err("refused")).expect("SetError JSON")
}

/// L256: "The server MUST reject, with `forbidden`, a `Chat/set` update on
/// such a Chat that sets `name`, `description`, or `avatarBlobId` or that
/// uses the `addMembers`, `removeMembers`, or `updateMemberRoles` patch
/// key".
#[test]
fn chat_set_update_on_room_chat_is_forbidden() {
    for key in [
        "name",
        "description",
        "avatarBlobId",
        "addMembers",
        "removeMembers",
        "updateMemberRoles",
    ] {
        assert_eq!(
            error(may_update_room_chat(&bound(), &["muted", key]))["type"],
            "forbidden",
            "{key}"
        );
    }
    assert!(may_update_room_chat(&bound(), &["muted"]).is_ok());
    assert!(may_update_room_chat(&extra(json!({})), &["name", "addMembers"]).is_ok());
}

/// L429: "The server MUST reject a `Message/set` create in a Chat with a
/// `roomId` with a `forbidden` SetError."
#[test]
fn message_create_in_room_chat_is_forbidden() {
    assert_eq!(
        error(may_create_room_message(&bound()))["type"],
        "forbidden"
    );
    assert!(may_create_room_message(&extra(json!({}))).is_ok());
}

/// L431: "The server MUST reject a `Message/set` update that changes
/// `body`, `bodyType`, or `reactions` of a Message in a Chat with a
/// `roomId` with an `invalidProperties` SetError naming the changed
/// properties."
#[test]
fn message_update_in_room_chat_names_changed_properties() {
    let err = error(may_update_room_message(
        &bound(),
        &["reactions/r1", "body", "readAt"],
    ));
    assert_eq!(err["type"], "invalidProperties");
    assert_eq!(err["properties"], json!(["body", "reactions"]));
    let err = error(may_update_room_message(&bound(), &["bodyType"]));
    assert_eq!(err["properties"], json!(["bodyType"]));
    assert!(may_update_room_message(&bound(), &["readAt"]).is_ok());
    assert!(may_update_room_message(&extra(json!({})), &["body"]).is_ok());
}
