//! Wire tests for the `Peer/*` method arguments of
//! draft-atwood-jmap-chat-federation-00 §Server-to-Server Methods.
//!
//! Oracle: hand-written JSON shaped by the quoted draft lines
//! (jmap-chat-spec draft-atwood-jmap-chat-federation-00.md). Each test
//! parses the JSON, checks the typed fields, and serializes back to the
//! same JSON.

use jmap_chat_federation_types::*;
use jmap_chat_types::{BodyType, ChatKind, ChatMemberRole, Clearable, Presence, ReadDisposition};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

/// Parse `wire`, then assert it serializes back unchanged.
fn round_trip<T: Serialize + DeserializeOwned>(wire: Value) -> T {
    let parsed: T = serde_json::from_value(wire.clone()).expect("valid wire JSON");
    assert_eq!(serde_json::to_value(&parsed).expect("serialize"), wire);
    parsed
}

/// L341-356: the `message` fields of Peer/deliver; L345: "Each entry
/// carries Attachment fields (as defined in {{JMAP-CHAT}}) plus `fetchUrl`
/// (String)"; L308: "the `id` of each Mention in `mentions`" carries an
/// identity string.
#[test]
fn deliver_message() {
    let args: PeerDeliverArgs = round_trip(json!({
        "accountId": "A1",
        "chatId": "01J0000000000000000000CHAT",
        "senderUserId": "alice@a.example",
        "chatKind": "group",
        "message": {
            "senderMsgId": "01J00000000000000000000MSG",
            "body": "hi @carol",
            "bodyType": "text/plain",
            "sentAt": "2026-10-20T10:00:00Z",
            "attachments": [{
                "blobId": "B1",
                "filename": "a.png",
                "contentType": "image/png",
                "size": 3,
                "sha256": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
                "fetchUrl": "https://a.example/jmap/download/A1/B1"
            }],
            "mentions": [{ "id": "carol@c.example" }],
            "broadcastMentions": [{ "scope": "here" }],
            "actions": [{ "type": "urn:example:poll", "uri": "https://a.example/p/1" }],
            "replyTo": "01J0000000000000000000PREV",
            "threadRootId": "01J0000000000000000000ROOT",
            "topicId": "t1",
            "subject": "Plans",
            "lastSeenSenderMsgId": "01J0000000000000000000PREV",
            "senderExpiresAt": "2026-10-21T10:00:00Z",
            "burnOnRead": true,
            "expiresAfterReadSeconds": 60
        }
    }));
    assert_eq!(args.sender_user_id, "alice@a.example");
    assert_eq!(args.chat_kind, ChatKind::Group);
    assert!(args.edit.is_none() && args.reaction_update.is_none());
    let m = args.message.expect("message");
    let att = &m.attachments[0];
    assert_eq!(att.fetch_url, "https://a.example/jmap/download/A1/B1");
    assert_eq!(att.attachment.size, 3);
    assert!(!att.attachment.extra.contains_key("fetchUrl"));
    assert_eq!(m.mentions.expect("mentions")[0].id, "carol@c.example");
    assert_eq!(m.burn_on_read, Some(true));
    assert_eq!(m.expires_after_read_seconds, Some(60));
}

/// L345: `attachments` is not marked optional, unlike its siblings.
#[test]
fn deliver_message_requires_attachments() {
    let wire = json!({
        "senderMsgId": "M1", "body": "x", "bodyType": "text/plain",
        "sentAt": "2026-10-20T10:00:00Z"
    });
    assert!(serde_json::from_value::<PeerMessage>(wire).is_err());
}

/// 29e9a41 L362: "`bodyType` (String) — MIME type of `body`", and L382
/// for the edit body. Both decode as the chat-types `BodyType`; an
/// unlisted MIME type such as `"application/mls-ciphertext"` (named at
/// L428) is kept as `Other` and serializes back unchanged.
#[test]
fn deliver_body_type_is_typed() {
    let m: PeerMessage = round_trip(json!({
        "senderMsgId": "M1", "body": "x", "bodyType": "application/jmap-chat-rich",
        "sentAt": "2026-10-20T10:00:00Z", "attachments": []
    }));
    assert_eq!(m.body_type, BodyType::Rich);
    let e: PeerEdit = round_trip(json!({
        "senderMsgId": "M1", "body": "AAEC", "bodyType": "application/mls-ciphertext",
        "editedAt": "2026-10-20T10:05:00Z"
    }));
    assert_eq!(
        e.body_type,
        BodyType::Other("application/mls-ciphertext".to_owned())
    );
}

/// L361-366: the `edit` fields of Peer/deliver.
#[test]
fn deliver_edit() {
    let args: PeerDeliverArgs = round_trip(json!({
        "accountId": "A1",
        "chatId": "C1",
        "senderUserId": "alice@a.example",
        "chatKind": "direct",
        "edit": {
            "senderMsgId": "M1",
            "body": "fixed",
            "bodyType": "text/markdown",
            "editedAt": "2026-10-20T10:05:00Z",
            "mentions": [{ "id": "bob@b.example" }]
        }
    }));
    let e = args.edit.expect("edit");
    assert_eq!(e.edited_at.as_ref(), "2026-10-20T10:05:00Z");
    assert_eq!(e.mentions.expect("mentions")[0].id, "bob@b.example");
}

/// L373-378: the `reactionUpdate` fields; L377: "`action` (String) —
/// `"add"` or `"remove"`."
#[test]
fn deliver_reaction_update() {
    let args: PeerDeliverArgs = round_trip(json!({
        "accountId": "A1",
        "chatId": "C1",
        "senderUserId": "alice@a.example",
        "chatKind": "channel",
        "reactionUpdate": {
            "senderMsgId": "M1",
            "senderReactionId": "R1",
            "emoji": "party",
            "customEmojiId": "E1",
            "action": "remove",
            "sentAt": "2026-10-20T10:06:00Z"
        }
    }));
    assert_eq!(
        args.reaction_update.expect("update").action,
        ReactionAction::Remove
    );
    assert_eq!(
        serde_json::from_value::<ReactionAction>(json!("add")).expect("add"),
        ReactionAction::Add
    );
    assert!(serde_json::from_value::<ReactionAction>(json!("toggle")).is_err());
}

/// L322: "Exactly one of `message`, `edit`, or `reactionUpdate` MUST be
/// present in a given request." Each constructor emits exactly one.
#[test]
fn deliver_constructors_emit_one_payload() {
    let msg = PeerMessage::new(
        "M1".into(),
        "x",
        "text/plain",
        "2026-10-20T10:00:00Z".into(),
        Vec::new(),
    );
    let args = PeerDeliverArgs::new_message(
        "A1".into(),
        "C1".into(),
        "alice@a.example",
        ChatKind::Direct,
        msg,
    );
    assert_eq!(
        serde_json::to_value(&args).expect("serialize"),
        json!({
            "accountId": "A1", "chatId": "C1", "senderUserId": "alice@a.example",
            "chatKind": "direct",
            "message": {
                "senderMsgId": "M1", "body": "x", "bodyType": "text/plain",
                "sentAt": "2026-10-20T10:00:00Z", "attachments": []
            }
        })
    );

    let edit = PeerEdit::new(
        "M1".into(),
        "y",
        "text/plain",
        "2026-10-20T10:01:00Z".into(),
    );
    let args = PeerDeliverArgs::new_edit(
        "A1".into(),
        "C1".into(),
        "a@a.example",
        ChatKind::Group,
        edit,
    );
    let wire = serde_json::to_value(&args).expect("serialize");
    assert!(wire.get("edit").is_some());
    assert!(wire.get("message").is_none() && wire.get("reactionUpdate").is_none());

    let update = PeerReactionUpdate::new(
        "M1".into(),
        "R1".into(),
        "party",
        ReactionAction::Add,
        "2026-10-20T10:02:00Z".into(),
    );
    let args = PeerDeliverArgs::new_reaction_update(
        "A1".into(),
        "C1".into(),
        "a@a.example",
        ChatKind::Group,
        update,
    );
    let wire = serde_json::to_value(&args).expect("serialize");
    assert_eq!(wire["reactionUpdate"]["action"], "add");
    assert!(wire.get("message").is_none() && wire.get("edit").is_none());
}

/// L429-439: the Peer/deliver response; `canonicalChatId` is "Absent in
/// all other cases".
#[test]
fn deliver_response() {
    let r: PeerDeliverResponse = round_trip(json!({
        "accountId": "A1",
        "receivedMsgId": "L1",
        "receivedAt": "2026-10-20T10:00:01Z",
        "canonicalChatId": "C0"
    }));
    assert_eq!(r.canonical_chat_id.as_ref().map(AsRef::as_ref), Some("C0"));

    let r = PeerDeliverResponse::new("A1".into(), "L1".into(), "2026-10-20T10:00:01Z".into());
    assert_eq!(
        serde_json::to_value(&r).expect("serialize"),
        json!({ "accountId": "A1", "receivedMsgId": "L1", "receivedAt": "2026-10-20T10:00:01Z" })
    );
}

/// L449-468: Peer/receipt arguments; L465: "Unrecognized values MUST be
/// stored as-is."
#[test]
fn receipt() {
    let a: PeerReceiptArgs = round_trip(json!({
        "accountId": "A1",
        "senderMsgId": "M1",
        "deliveredAt": "2026-10-20T10:00:01Z",
        "deviceDeliveredAt": "2026-10-20T10:00:02Z",
        "readAt": "2026-10-20T10:01:00Z",
        "readDisposition": "displayed",
        "readerUserId": "bob@b.example"
    }));
    assert_eq!(a.read_disposition, Some(ReadDisposition::Displayed));
    assert_eq!(a.reader_user_id, "bob@b.example");

    let a: PeerReceiptArgs = round_trip(json!({
        "accountId": "A1",
        "senderMsgId": "M1",
        "readAt": "2026-10-20T10:01:00Z",
        "readDisposition": "x-skimmed",
        "readerUserId": "bob@b.example"
    }));
    assert_eq!(
        a.read_disposition,
        Some(ReadDisposition::Other("x-skimmed".into()))
    );
}

/// L511-521: Peer/typing; L520: "`typing` (Boolean)" is required.
#[test]
fn typing() {
    let a: PeerTypingArgs = round_trip(json!({
        "accountId": "A1", "chatId": "C1", "senderUserId": "bob@b.example", "typing": false
    }));
    assert!(!a.typing);
    let missing = json!({ "accountId": "A1", "chatId": "C1", "senderUserId": "bob@b.example" });
    assert!(serde_json::from_value::<PeerTypingArgs>(missing).is_err());
}

/// L566-594: Peer/retract arguments and response.
#[test]
fn retract() {
    let a: PeerRetractArgs = round_trip(json!({
        "accountId": "A1", "chatId": "C1", "senderUserId": "alice@a.example", "senderMsgId": "M1"
    }));
    assert_eq!(a.sender_msg_id, "M1");
    let r: PeerRetractResponse = round_trip(json!({
        "accountId": "A1", "retractedAt": "2026-10-20T11:00:00Z"
    }));
    assert_eq!(r.retracted_at.as_ref(), "2026-10-20T11:00:00Z");
}

/// L616-622: the seven `action` values of Peer/groupUpdate; any other
/// value is not an action.
#[test]
fn group_update_actions() {
    for (wire, action) in [
        ("create", GroupUpdateAction::Create),
        ("addMembers", GroupUpdateAction::AddMembers),
        ("removeMembers", GroupUpdateAction::RemoveMembers),
        ("updateRoles", GroupUpdateAction::UpdateRoles),
        ("updateMetadata", GroupUpdateAction::UpdateMetadata),
        ("leave", GroupUpdateAction::Leave),
        ("close", GroupUpdateAction::Close),
    ] {
        let parsed: GroupUpdateAction = serde_json::from_value(json!(wire)).expect(wire);
        assert_eq!(parsed, action);
        assert_eq!(
            serde_json::to_value(action).expect("serialize"),
            json!(wire)
        );
    }
    assert!(serde_json::from_value::<GroupUpdateAction>(json!("rename")).is_err());
}

/// L624-641 with L308: "the `id` and `invitedBy` of each ChatMember in
/// `members` and `addedMembers`" carry identity strings.
#[test]
fn group_update_create() {
    let a: PeerGroupUpdateArgs = round_trip(json!({
        "accountId": "A1",
        "chatId": "01J000000000000000000GROUP",
        "senderUserId": "alice@a.example",
        "action": "create",
        "members": [
            { "id": "alice@a.example", "role": "admin", "joinedAt": "2026-10-20T09:00:00Z" },
            {
                "id": "bob@b.example", "role": "member", "joinedAt": "2026-10-20T09:00:00Z",
                "invitedBy": "alice@a.example"
            }
        ],
        "name": "Trip",
        "description": "Planning",
        "avatar": {
            "contentType": "image/png",
            "size": 3,
            "sha256": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "fetchUrl": "https://a.example/blob/av1"
        }
    }));
    let members = a.members.expect("members");
    assert_eq!(members[1].id, "bob@b.example");
    assert_eq!(members[1].invited_by.as_deref(), Some("alice@a.example"));
    assert_eq!(members[0].role, ChatMemberRole::Admin);
    let Some(Clearable::Set(avatar)) = a.avatar else {
        panic!("avatar object");
    };
    assert_eq!(avatar.content_type, "image/png");
    assert_eq!(avatar.size, 3);
    assert_eq!(avatar.fetch_url, "https://a.example/blob/av1");
}

/// fed 6bcd9d8 L667-668: `avatar` (Object|null, optional): "For
/// `"updateMetadata"`, `null` removes the group avatar; absent leaves it
/// unchanged." The two must stay distinct.
#[test]
fn group_update_avatar_null_and_absent_differ() {
    let a: PeerGroupUpdateArgs = round_trip(json!({
        "accountId": "A1", "chatId": "G1", "senderUserId": "alice@a.example",
        "action": "updateMetadata", "avatar": null
    }));
    assert!(matches!(a.avatar, Some(Clearable::Clear)));
    assert!(a.extra.is_empty());

    let a: PeerGroupUpdateArgs = round_trip(json!({
        "accountId": "A1", "chatId": "G1", "senderUserId": "alice@a.example",
        "action": "updateMetadata", "name": "Trip"
    }));
    assert!(a.avatar.is_none());
}

/// L627-635: `addedMembers`, `removedMemberIds` ("The identity strings of
/// the members who were removed") and `updatedRoles` ("Each entry MUST
/// contain `id` ... and `role`").
#[test]
fn group_update_membership_fields() {
    let a: PeerGroupUpdateArgs = round_trip(json!({
        "accountId": "A1", "chatId": "G1", "senderUserId": "alice@a.example",
        "action": "addMembers",
        "addedMembers": [
            { "id": "dave@d.example", "role": "member", "joinedAt": "2026-10-20T09:30:00Z" }
        ]
    }));
    assert_eq!(a.added_members.expect("added")[0].id, "dave@d.example");

    let a: PeerGroupUpdateArgs = round_trip(json!({
        "accountId": "A1", "chatId": "G1", "senderUserId": "alice@a.example",
        "action": "removeMembers", "removedMemberIds": ["dave@d.example"]
    }));
    assert_eq!(
        a.removed_member_ids,
        Some(vec!["dave@d.example".to_owned()])
    );

    let a: PeerGroupUpdateArgs = round_trip(json!({
        "accountId": "A1", "chatId": "G1", "senderUserId": "alice@a.example",
        "action": "updateRoles", "updatedRoles": [{ "id": "bob@b.example", "role": "admin" }]
    }));
    let roles = a.updated_roles.expect("roles");
    assert_eq!(roles[0].id, "bob@b.example");
    assert_eq!(roles[0].role, ChatMemberRole::Admin);
    assert!(serde_json::from_value::<PeerRoleUpdate>(json!({ "id": "bob@b.example" })).is_err());
}

/// L670-700: Peer/subscribePresence; the response's `ttl` (UnsignedInt)
/// is required, the request's is optional.
#[test]
fn subscribe_presence() {
    let a: PeerSubscribePresenceArgs = round_trip(json!({
        "accountId": "A1", "subscriberId": "bob@b.example", "ttl": 3600
    }));
    assert_eq!(a.ttl, Some(3600));
    let r: PeerSubscribePresenceResponse = round_trip(json!({
        "accountId": "A1",
        "presence": "busy",
        "lastActiveAt": "2026-10-20T08:00:00Z",
        "statusText": "In a meeting",
        "statusEmoji": "calendar",
        "ttl": 1800
    }));
    assert_eq!(r.presence, Presence::Busy);
    assert_eq!(r.ttl, 1800);
    let no_ttl = json!({ "accountId": "A1", "presence": "online" });
    assert!(serde_json::from_value::<PeerSubscribePresenceResponse>(no_ttl).is_err());
}

/// L713-717: Peer/unsubscribePresence arguments.
#[test]
fn unsubscribe_presence() {
    let a: PeerUnsubscribePresenceArgs =
        round_trip(json!({ "accountId": "A1", "subscriberId": "bob@b.example" }));
    assert_eq!(a.subscriber_id, "bob@b.example");
}

/// L745: "If absent, the receiver MUST treat the current value as
/// unchanged. To explicitly clear the field, the sender MUST send `null`."
#[test]
fn presence_status_absent_null_and_set() {
    let a: PeerPresenceArgs = round_trip(json!({
        "accountId": "A1", "identity": "alice@a.example", "presence": "away"
    }));
    assert_eq!(a.status_text, None);
    assert_eq!(a.status_emoji, None);

    let a: PeerPresenceArgs = round_trip(json!({
        "accountId": "A1", "identity": "alice@a.example", "presence": "offline",
        "lastActiveAt": "2026-10-20T08:00:00Z",
        "statusText": null, "statusEmoji": "coffee"
    }));
    assert_eq!(a.presence, Presence::Offline);
    assert_eq!(a.status_text, Some(Clearable::Clear));
    assert_eq!(a.status_emoji, Some(Clearable::Set("coffee".to_owned())));
}

/// L476, L525, L655, L721, L752: these responses carry `accountId` only.
#[test]
fn ack_response() {
    let r: PeerAck = round_trip(json!({ "accountId": "A1" }));
    assert_eq!(r.account_id, "A1");
    assert_eq!(
        serde_json::to_value(PeerAck::new("A1".into())).expect("serialize"),
        json!({ "accountId": "A1" })
    );
}

/// Workspace extras-preservation policy (AGENTS.md): unknown fields on
/// wire structs survive a round trip, at every nesting level.
#[test]
fn unknown_fields_survive() {
    let a: PeerDeliverArgs = round_trip(json!({
        "accountId": "A1", "chatId": "C1", "senderUserId": "a@a.example",
        "chatKind": "direct", "x-trace": "t1",
        "message": {
            "senderMsgId": "M1", "body": "x", "bodyType": "text/plain",
            "sentAt": "2026-10-20T10:00:00Z", "x-prio": 1,
            "attachments": [{
                "blobId": "B1", "filename": "f", "contentType": "text/plain", "size": 0,
                "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                "fetchUrl": "https://a.example/b", "x-hint": "cold"
            }],
            "mentions": [{ "id": "b@b.example", "x-m": true }]
        }
    }));
    assert_eq!(a.extra["x-trace"], "t1");
    let m = a.message.expect("message");
    assert_eq!(m.extra["x-prio"], 1);
    assert_eq!(m.attachments[0].attachment.extra["x-hint"], "cold");
    assert_eq!(m.mentions.expect("mentions")[0].extra["x-m"], true);

    let g: PeerGroupUpdateArgs = round_trip(json!({
        "accountId": "A1", "chatId": "G1", "senderUserId": "a@a.example",
        "action": "leave", "x-reason": "bye"
    }));
    assert_eq!(g.extra["x-reason"], "bye");
}
