//! `ChatMessagePush` payload construction (bd:JMAP-nbk3.6).
//!
//! Oracle: draft-atwood-jmap-chat-push-00 at jmap-chat-spec commit cefc217,
//! sections "ChatPushConfig", "ChatMessagePush", "ChatMessageEntry",
//! "End-to-End Encrypted Deployments" and "Urgency", plus the approved
//! JMAP-nbk3 plan decisions D3 (`ChatMessageEntry` fields are optional so
//! `properties` can be honoured), D8 (`bodySnippet`: control characters
//! stripped, then truncated; omitted for `application/jmap-chat-rich`) and
//! D9 (an empty `login` counts as absent). Each test quotes the sentence it
//! enforces. Fixtures are draft-shaped JSON, not values produced by the
//! code under test.

use jmap_chat_server::{build_chat_message_push, BuiltPush, OwnerFlags, PushItem};
use jmap_chat_types::{
    Chat, ChatContact, ChatMessagePush, ChatPushCapability, ChatPushConfig, Message, Space,
    SpaceMember, UrgencyLevel,
};
use jmap_types::{Id, State};
use serde_json::{json, Value};

const ACCOUNT: &str = "acct-owner";
const SENDER: &str = "contact-bob";
const CHAT: &str = "chat-1";
const SPACE: &str = "space-1";
const SENT_AT: &str = "2026-10-19T12:00:00Z";
const ENCRYPTED_BODY_TYPE: &str = "message/mls";

const NOBODY: OwnerFlags = OwnerFlags {
    active: false,
    admin: false,
};

fn id(s: &str) -> Id {
    serde_json::from_value(json!(s)).expect("valid Id")
}

fn state(s: &str) -> State {
    serde_json::from_value(json!(s)).expect("valid State")
}

/// The account advertises `maxSnippetBytes: 256` and the urgency values
/// used below.
fn cap() -> ChatPushCapability {
    serde_json::from_value(json!({
        "maxSnippetBytes": 256,
        "supportedUrgencyValues": ["low", "normal", "high"],
    }))
    .expect("valid ChatPushCapability")
}

fn config(v: Value) -> ChatPushConfig {
    serde_json::from_value(v).expect("valid ChatPushConfig")
}

fn message(body: &str, body_type: &str, mentions: Value, broadcast: Value) -> Message {
    serde_json::from_value(json!({
        "id": "msg-1",
        "senderMsgId": "smsg-1",
        "senderId": SENDER,
        "chatId": CHAT,
        "body": body,
        "bodyType": body_type,
        "attachments": [],
        "mentions": mentions,
        "broadcastMentions": broadcast,
        "sentAt": SENT_AT,
        "receivedAt": "2026-10-19T12:00:01Z",
        "deliveryState": "received",
    }))
    .expect("valid Message")
}

fn plain(body: &str) -> Message {
    message(body, "text/plain", json!([]), json!([]))
}

fn group_chat() -> Chat {
    serde_json::from_value(json!({
        "id": CHAT,
        "kind": "group",
        "createdAt": "2026-10-01T00:00:00Z",
        "unreadCount": 0,
        "name": "Team",
    }))
    .expect("valid Chat")
}

fn channel_chat() -> Chat {
    serde_json::from_value(json!({
        "id": CHAT,
        "kind": "channel",
        "createdAt": "2026-10-01T00:00:00Z",
        "unreadCount": 0,
        "name": "general",
        "spaceId": SPACE,
    }))
    .expect("valid Chat")
}

fn contact(display_name: Option<&str>, login: &str) -> ChatContact {
    let mut v = json!({
        "id": SENDER,
        "identity": "bob@example.net",
        "login": login,
        "firstSeenAt": "2026-10-01T00:00:00Z",
        "lastSeenAt": "2026-10-19T00:00:00Z",
    });
    if let Some(name) = display_name {
        v["displayName"] = json!(name);
    }
    serde_json::from_value(v).expect("valid ChatContact")
}

fn bob() -> ChatContact {
    contact(Some("Bob"), "bob")
}

fn member(nick: Option<&str>) -> SpaceMember {
    let mut v = json!({
        "id": SENDER,
        "roleIds": [],
        "joinedAt": "2026-10-01T00:00:00Z",
    });
    if let Some(nick) = nick {
        v["nick"] = json!(nick);
    }
    serde_json::from_value(v).expect("valid SpaceMember")
}

/// Build a payload for `msg` in `chat` from `sender`, with the default
/// capability and state `"s42"`.
fn build(
    cfg: &ChatPushConfig,
    owner: OwnerFlags,
    msg: &Message,
    chat: &Chat,
    sender: &ChatContact,
    sender_member: Option<&SpaceMember>,
) -> BuiltPush {
    build_chat_message_push(
        &id(ACCOUNT),
        state("s42"),
        cfg,
        &cap(),
        owner,
        PushItem {
            message: msg,
            chat,
            sender,
            sender_member,
            space: None,
        },
    )
}

/// The single entry of `push`, as wire JSON.
fn entry_json(push: &ChatMessagePush) -> serde_json::Map<String, Value> {
    let wire = serde_json::to_value(push).expect("serialize ChatMessagePush");
    let messages = wire["messages"].as_array().expect("messages array");
    assert_eq!(messages.len(), 1, "one message per payload");
    messages[0].as_object().expect("entry object").clone()
}

fn keys(entry: &serde_json::Map<String, Value>) -> Vec<String> {
    let mut k: Vec<String> = entry.keys().cloned().collect();
    k.sort();
    k
}

fn sorted(v: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = v.iter().map(|s| (*s).to_owned()).collect();
    v.sort();
    v
}

/// draft-atwood-jmap-chat-push-00 §ChatPushConfig (push:121): "The
/// `properties` list is authoritative: the server returns exactly the
/// requested fields." A client using this crate's own types must still be
/// able to read such a payload (plan decision D3).
#[test]
fn properties_message_id_only_gives_exactly_one_key() {
    let cfg = config(json!({ "properties": ["messageId"] }));
    let built = build(&cfg, NOBODY, &plain("hello"), &group_chat(), &bob(), None);
    let entry = entry_json(&built.payload);
    assert_eq!(keys(&entry), vec!["messageId".to_owned()]);
    assert_eq!(entry["messageId"], json!("msg-1"));

    let wire = serde_json::to_value(&built.payload).expect("serialize");
    let parsed: ChatMessagePush =
        serde_json::from_value(wire).expect("client deserializes a one-key entry");
    assert_eq!(parsed.messages.len(), 1);
    assert_eq!(parsed.messages[0].message_id, Some(id("msg-1")));
    assert_eq!(parsed.messages[0].chat_id, None);
    assert_eq!(parsed.messages[0].encrypted, None);
}

/// draft-atwood-jmap-chat-push-00 §ChatPushConfig (push:121): "If absent,
/// the server uses the default set: `messageId`, `chatId`, `chatKind`,
/// `senderId`, `senderDisplayName`, `sentAt`, `hasMention`,
/// `mentionScopes`, `encrypted`, and `bodySnippet`."
#[test]
fn absent_properties_gives_default_set() {
    let cfg = config(json!({}));
    let built = build(
        &cfg,
        NOBODY,
        &plain("hello"),
        &channel_chat(),
        &bob(),
        Some(&member(Some("bobby"))),
    );
    let entry = entry_json(&built.payload);
    assert_eq!(
        keys(&entry),
        sorted(&[
            "messageId",
            "chatId",
            "chatKind",
            "senderId",
            "senderDisplayName",
            "sentAt",
            "hasMention",
            "mentionScopes",
            "encrypted",
            "bodySnippet",
        ])
    );
    assert_eq!(entry["messageId"], json!("msg-1"));
    assert_eq!(entry["chatId"], json!(CHAT));
    assert_eq!(entry["chatKind"], json!("channel"));
    // push:170: "The accountId for owner-composed messages; otherwise the
    // ChatContact.id of the message sender."
    assert_eq!(entry["senderId"], json!(SENDER));
    // push:176: "The sender's claimed composition time, as stored in the
    // message."
    assert_eq!(entry["sentAt"], json!(SENT_AT));
    assert_eq!(entry["encrypted"], json!(false));
    assert_eq!(entry["bodySnippet"], json!("hello"));
}

/// draft-atwood-jmap-chat-push-00 §ChatMessageEntry (push:161, 164):
/// `chatName` is "The display name of the Chat. Present for `"group"` and
/// `"channel"` kinds."; `spaceId` is "For `"channel"` chats, the id of the
/// containing Space."
#[test]
fn requested_chat_name_and_space_id_for_channel() {
    let cfg = config(json!({ "properties": ["chatName", "spaceId"] }));
    let built = build(&cfg, NOBODY, &plain("hi"), &channel_chat(), &bob(), None);
    let entry = entry_json(&built.payload);
    assert_eq!(keys(&entry), sorted(&["chatName", "spaceId"]));
    assert_eq!(entry["chatName"], json!("general"));
    assert_eq!(entry["spaceId"], json!(SPACE));
}

fn acme_space() -> Space {
    serde_json::from_value(json!({
        "id": SPACE,
        "name": "ACME Corp",
        "roles": [],
        "members": [],
        "categories": [],
        "uncategorizedChannelIds": [CHAT],
        "createdAt": "2026-10-01T00:00:00Z",
        "isPublic": false,
        "isPubliclyPreviewable": false,
        "memberCount": 0,
    }))
    .expect("valid Space")
}

/// Build a payload for a plain message in `chat` with `space` passed as
/// the containing Space.
fn build_with_space(cfg: &ChatPushConfig, chat: &Chat, space: &Space) -> BuiltPush {
    build_chat_message_push(
        &id(ACCOUNT),
        state("s42"),
        cfg,
        &cap(),
        NOBODY,
        PushItem {
            message: &plain("hi"),
            chat,
            sender: &bob(),
            sender_member: None,
            space: Some(space),
        },
    )
}

/// draft-atwood-jmap-chat-push-00 §ChatPushConfig (push:121): "The server
/// MUST support at minimum the following properties: ... `spaceId`,
/// `spaceName`, ..."; §ChatMessageEntry (push:166): `spaceName` is "For
/// `"channel"` chats, the display name of the containing Space."
#[test]
fn requested_space_name_for_channel() {
    let cfg = config(json!({ "properties": ["spaceName"] }));
    let built = build_with_space(&cfg, &channel_chat(), &acme_space());
    let entry = entry_json(&built.payload);
    assert_eq!(keys(&entry), vec!["spaceName".to_owned()]);
    assert_eq!(entry["spaceName"], json!("ACME Corp"));
}

/// draft-atwood-jmap-chat-push-00 §ChatMessageEntry (push:166-167):
/// `spaceName` is "Absent for other kinds."
#[test]
fn space_name_absent_for_group_even_when_requested() {
    let cfg = config(json!({ "properties": ["spaceName", "chatName"] }));
    let built = build_with_space(&cfg, &group_chat(), &acme_space());
    let entry = entry_json(&built.payload);
    assert_eq!(keys(&entry), vec!["chatName".to_owned()]);
}

/// draft-atwood-jmap-chat-push-00 §ChatMessagePush (push:142): "The server
/// MUST set this field to the post-delivery state". With one message per
/// payload, that is the state the caller passes for this message.
#[test]
fn state_and_account_match_input() {
    let cfg = config(json!({}));
    let built = build(&cfg, NOBODY, &plain("hi"), &group_chat(), &bob(), None);
    assert_eq!(built.payload.state, state("s42"));
    assert_eq!(built.payload.account_id, id(ACCOUNT));
    let wire = serde_json::to_value(&built.payload).expect("serialize");
    assert_eq!(wire["@type"], json!("ChatMessagePush"));
    assert_eq!(wire["state"], json!("s42"));
}

/// draft-atwood-jmap-chat-push-00 §ChatMessageEntry (push:179): "The server
/// MUST set this to `true` when the accountId, which identifies the account
/// owner ({{JMAP-CHAT}}), appears as a Mention `id` in the message's
/// `Message.mentions` array, for every `bodyType`. The server MUST NOT
/// derive mentions from the message body".
#[test]
fn has_mention_comes_only_from_mentions_ids() {
    let cfg = config(json!({ "properties": ["hasMention"] }));
    let chat = group_chat();
    let owner = OwnerFlags {
        active: true,
        admin: true,
    };

    let broadcast_only = message(
        "@everyone hi",
        "text/plain",
        json!([]),
        json!([{ "scope": "everyone" }]),
    );
    let e = entry_json(&build(&cfg, owner, &broadcast_only, &chat, &bob(), None).payload);
    assert_eq!(e["hasMention"], json!(false), "broadcast-only mention");

    let body_only = plain("hey @acct-owner");
    let e = entry_json(&build(&cfg, owner, &body_only, &chat, &bob(), None).payload);
    assert_eq!(e["hasMention"], json!(false), "body text is not a mention");

    let other = message(
        "hi",
        "text/plain",
        json!([{ "id": "acct-other" }]),
        json!([]),
    );
    let e = entry_json(&build(&cfg, owner, &other, &chat, &bob(), None).payload);
    assert_eq!(e["hasMention"], json!(false), "someone else mentioned");

    let direct = message("hi", "text/plain", json!([{ "id": ACCOUNT }]), json!([]));
    let e = entry_json(&build(&cfg, owner, &direct, &chat, &bob(), None).payload);
    assert_eq!(e["hasMention"], json!(true), "owner mentioned");

    let rich = message(
        "{}",
        "application/jmap-chat-rich",
        json!([{ "id": ACCOUNT }]),
        json!([]),
    );
    let e = entry_json(&build(&cfg, owner, &rich, &chat, &bob(), None).payload);
    assert_eq!(
        e["hasMention"],
        json!(true),
        "rich body, mentions[] still counts"
    );
}

/// draft-atwood-jmap-chat-push-00 §ChatMessageEntry (push:182): "The server
/// MUST compute the candidate scopes as the deduplicated `scope` values
/// appearing in `Message.broadcastMentions[]` ... `"everyone"` is included
/// whenever the owner is a member of the Chat; `"here"` is included only
/// when the owner satisfies the deployment-defined "active" predicate at
/// delivery time; `"admins"` is included only when the owner satisfies the
/// deployment-defined "administrative authority" predicate at delivery
/// time."
#[test]
fn mention_scopes_filtered_by_owner_flags_and_deduplicated() {
    let cfg = config(json!({ "properties": ["mentionScopes"] }));
    let msg = message(
        "hi all",
        "text/plain",
        json!([]),
        json!([
            { "scope": "everyone" },
            { "scope": "here" },
            { "scope": "admins" },
            { "scope": "everyone" },
        ]),
    );
    let chat = group_chat();
    let scopes = |owner: OwnerFlags| -> Vec<String> {
        let e = entry_json(&build(&cfg, owner, &msg, &chat, &bob(), None).payload);
        let mut v: Vec<String> = serde_json::from_value(e["mentionScopes"].clone())
            .expect("mentionScopes is a string array");
        v.sort();
        v
    };
    assert_eq!(scopes(NOBODY), sorted(&["everyone"]));
    assert_eq!(
        scopes(OwnerFlags {
            active: true,
            admin: false
        }),
        sorted(&["everyone", "here"])
    );
    assert_eq!(
        scopes(OwnerFlags {
            active: false,
            admin: true
        }),
        sorted(&["everyone", "admins"])
    );

    let none = plain("hi");
    let e = entry_json(&build(&cfg, NOBODY, &none, &chat, &bob(), None).payload);
    // push:182: "The empty array means either that no broadcast scopes were
    // used or that the owner was not targeted by any of them."
    assert_eq!(e["mentionScopes"], json!([]));
}

/// draft-atwood-jmap-chat-push-00 §ChatMessageEntry (push:188): "At most
/// `maxSnippetBytes` bytes, truncated on a UTF-8 character boundary."
/// U+20AC (0xE282AC, three bytes) at byte offset 255 straddles the 256-byte
/// cap and must be dropped whole.
#[test]
fn snippet_truncates_on_char_boundary() {
    let cfg = config(json!({ "properties": ["bodySnippet"] }));
    let chat = group_chat();
    let a255 = "a".repeat(255);

    let straddle = plain(&format!("{a255}\u{20AC}"));
    assert_eq!(straddle.body.len(), 258);
    let e = entry_json(&build(&cfg, NOBODY, &straddle, &chat, &bob(), None).payload);
    assert_eq!(e["bodySnippet"], json!(a255));

    let a253 = "a".repeat(253);
    let exact = plain(&format!("{a253}\u{20AC}"));
    assert_eq!(exact.body.len(), 256);
    let e = entry_json(&build(&cfg, NOBODY, &exact, &chat, &bob(), None).payload);
    assert_eq!(
        e["bodySnippet"],
        json!(format!("{a253}\u{20AC}")),
        "a body of exactly maxSnippetBytes is kept whole"
    );
}

/// draft-atwood-jmap-chat-push-00 §ChatMessageEntry (push:188): "A
/// truncated plaintext rendering of the message body, suitable for display
/// in a notification preview." Plan decision D8: control characters are
/// stripped before truncation, so a stripped BEL frees a byte of the cap.
#[test]
fn snippet_strips_control_characters_before_truncating() {
    let cfg = config(json!({ "properties": ["bodySnippet"] }));
    let body = format!("\u{7}{}b", "a".repeat(255));
    assert_eq!(body.len(), 257);
    let e = entry_json(&build(&cfg, NOBODY, &plain(&body), &group_chat(), &bob(), None).payload);
    assert_eq!(e["bodySnippet"], json!(format!("{}b", "a".repeat(255))));
}

/// draft-atwood-jmap-chat-push-00 §End-to-End Encrypted Deployments
/// (push:197): "For the plaintext body types defined by {{JMAP-CHAT}} —
/// `"text/plain"`, `"text/markdown"`, and `"application/jmap-chat-rich"` —
/// the server MUST set `encrypted` to `false`." Plan decision D8 omits the
/// snippet for rich bodies (a documented departure from push:121).
#[test]
fn plaintext_body_types_are_not_encrypted() {
    let cfg = config(json!({ "properties": ["encrypted", "bodySnippet"] }));
    let chat = group_chat();

    let md = message("**hi**", "text/markdown", json!([]), json!([]));
    let e = entry_json(&build(&cfg, NOBODY, &md, &chat, &bob(), None).payload);
    assert_eq!(e["encrypted"], json!(false));
    assert!(e.contains_key("bodySnippet"), "markdown gets a snippet");

    let rich = message("{}", "application/jmap-chat-rich", json!([]), json!([]));
    let e = entry_json(&build(&cfg, NOBODY, &rich, &chat, &bob(), None).payload);
    assert_eq!(e["encrypted"], json!(false));
    assert!(!e.contains_key("bodySnippet"), "D8: rich snippet omitted");
}

/// draft-atwood-jmap-chat-push-00 §End-to-End Encrypted Deployments
/// (push:197): "For all other `bodyType` values, the server MUST set
/// `encrypted` to `true` unless it has explicit knowledge that the type
/// represents a plaintext format. The server MUST omit `bodySnippet` when
/// `encrypted` is `true`". (push:199): "The server MUST omit both fields
/// from the entry when `encrypted` is `true`." (push:201): "The remaining
/// fields (`messageId`, `chatId`, `chatKind`, `chatName`, `senderId`,
/// `senderDisplayName`, `sentAt`, and `encrypted`) carry enough context".
#[test]
fn encrypted_entry_omits_content_and_mentions() {
    let cfg = config(json!({}));
    let msg = message(
        "Y2lwaGVydGV4dA",
        ENCRYPTED_BODY_TYPE,
        json!([{ "id": ACCOUNT }]),
        json!([{ "scope": "everyone" }]),
    );
    let e = entry_json(&build(&cfg, NOBODY, &msg, &group_chat(), &bob(), None).payload);
    assert_eq!(e["encrypted"], json!(true));
    assert_eq!(
        keys(&e),
        sorted(&[
            "messageId",
            "chatId",
            "chatKind",
            "senderId",
            "senderDisplayName",
            "sentAt",
            "encrypted",
        ])
    );
}

/// draft-atwood-jmap-chat-push-00 §ChatMessageEntry (push:173): "For
/// `chatKind: "channel"`, derived from the sender's `SpaceMember.nick` in
/// the containing Space if present, otherwise `ChatContact.displayName` if
/// present, otherwise `ChatContact.login`, otherwise
/// `ChatContact.identity`. For `"direct"` and `"group"` chats, derived from
/// `ChatContact.displayName` if present, otherwise `ChatContact.login`,
/// otherwise `ChatContact.identity`." Plan decision D9: an empty `login`
/// counts as absent.
#[test]
fn sender_display_name_fallback_chain() {
    let cfg = config(json!({ "properties": ["senderDisplayName"] }));
    let msg = plain("hi");
    let name = |chat: &Chat, sender: &ChatContact, m: Option<&SpaceMember>| -> Value {
        entry_json(&build(&cfg, NOBODY, &msg, chat, sender, m).payload)["senderDisplayName"].clone()
    };
    let channel = channel_chat();
    let group = group_chat();
    let nick = member(Some("bobby"));
    let no_nick = member(None);

    assert_eq!(name(&channel, &bob(), Some(&nick)), json!("bobby"));
    assert_eq!(name(&channel, &bob(), Some(&no_nick)), json!("Bob"));
    assert_eq!(
        name(&group, &bob(), Some(&nick)),
        json!("Bob"),
        "nick is channel-only"
    );
    assert_eq!(name(&group, &contact(None, "bob"), None), json!("bob"));
    assert_eq!(
        name(&group, &contact(None, ""), None),
        json!("bob@example.net")
    );
}

/// draft-atwood-jmap-chat-push-00 §Urgency (push:223): "The server uses
/// `urgency` as the Web Push urgency for the delivery. If the
/// `ChatPushConfig` specifies a `mentionUrgency` and at least one entry in
/// `messages` has `hasMention: true` or a non-empty `mentionScopes`, the
/// server MUST use `mentionUrgency` for the entire payload instead."
/// (push:124): "Default is `"normal"`." (push:199): an encrypted message
/// "never triggers `mentionUrgency`".
#[test]
fn urgency_selection() {
    let chat = group_chat();
    let both = config(json!({ "urgency": "low", "mentionUrgency": "high" }));
    let plain_msg = plain("hi");
    let direct = message("hi", "text/plain", json!([{ "id": ACCOUNT }]), json!([]));
    let broadcast = message(
        "hi",
        "text/plain",
        json!([]),
        json!([{ "scope": "everyone" }]),
    );
    let encrypted = message(
        "x",
        ENCRYPTED_BODY_TYPE,
        json!([{ "id": ACCOUNT }]),
        json!([]),
    );

    let urgency =
        |cfg: &ChatPushConfig, msg: &Message| build(cfg, NOBODY, msg, &chat, &bob(), None).urgency;
    assert_eq!(urgency(&both, &plain_msg), UrgencyLevel::Low);
    assert_eq!(urgency(&both, &direct), UrgencyLevel::High);
    assert_eq!(urgency(&both, &broadcast), UrgencyLevel::High);
    assert_eq!(urgency(&both, &encrypted), UrgencyLevel::Low);

    let only_urgency = config(json!({ "urgency": "low" }));
    assert_eq!(urgency(&only_urgency, &direct), UrgencyLevel::Low);

    let empty = config(json!({}));
    assert_eq!(urgency(&empty, &plain_msg), UrgencyLevel::Normal);
    let only_mention = config(json!({ "mentionUrgency": "high" }));
    assert_eq!(urgency(&only_mention, &plain_msg), UrgencyLevel::Normal);
    assert_eq!(urgency(&only_mention, &direct), UrgencyLevel::High);
}
