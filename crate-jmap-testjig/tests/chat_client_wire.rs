//! Wire-compat tests: the published `jmap-chat-client` API driven against
//! the real `jmap-chat-server` reference backend served by the in-process
//! jig.
//!
//! Every `jmap-chat-client` test runs against wiremock with hand-written
//! responses; these tests instead discover the Session over HTTP, follow
//! its `apiUrl`, and let the real dispatcher + `MemoryBackend` answer.
//! Each assertion's oracle is a sentence of draft-atwood-jmap-chat-00 or
//! RFC 8620, quoted in the test's doc comment — never the client or the
//! server under test.

use jmap_base_client::{BearerAuth, ClientConfig, DefaultTransport, JmapClient, Session};
use jmap_chat_client::{
    ChatCreateInput, ChatSessionExt, ClientError, JmapChatExt, MessageCreateInput, MessagePatch,
    MessageQueryInput, ReactionChange, SessionClient, SetResponse, SpaceBanCreateInput,
    SpaceCreateInput, SpaceJoinInput, SpacePatch,
};
use jmap_chat_types::{Attachment, BodyType, ChatKind, Mention, MessageAction, SenderId};
use jmap_testjig::{spawn_in_process, TestjigConfig, TestjigHandle};
use jmap_types::{Id, UTCDate};

const SENT_AT: &str = "2026-01-01T00:00:00Z";

/// Spawn a jig and build a client the way an application would: from
/// the server origin alone, discovering everything else via the Session.
async fn connect() -> (TestjigHandle, JmapClient, Session) {
    let jig = spawn_in_process(TestjigConfig::default())
        .await
        .expect("spawn jig");
    let client = JmapClient::new(
        DefaultTransport,
        BearerAuth::new(&jig.token).expect("bearer token"),
        &format!("http://{}", jig.addr),
        ClientConfig::default(),
    )
    .expect("JmapClient::new");
    let session = client.fetch_session().await.expect("fetch_session");
    (jig, client, session)
}

/// The server-assigned id for creation key `key`, or a panic naming the
/// SetError the server returned instead.
fn created_id(resp: &SetResponse, key: &str) -> Id {
    if let Some(err) = resp.not_created.as_ref().and_then(|m| m.get(key)) {
        panic!("{key} not created: {err:?}");
    }
    let id = resp
        .created
        .as_ref()
        .and_then(|m| m.get(key))
        .and_then(|v| v["id"].as_str())
        .unwrap_or_else(|| panic!("created[{key}].id missing: {resp:?}"));
    Id::from(id)
}

async fn create_group(sc: &SessionClient, key: &str, name: &str) -> Id {
    let members = [Id::from("contact-bob")];
    let resp = sc
        .chat_create(&ChatCreateInput::Group {
            client_id: Some(key),
            name,
            member_ids: &members,
            description: None,
            avatar_blob_id: None,
            message_expiry_seconds: None,
        })
        .await
        .expect("Chat/set group create");
    created_id(&resp, key)
}

async fn send(sc: &SessionClient, chat_id: &Id, key: &str, body: &str) -> Id {
    let sent_at = UTCDate::from(SENT_AT);
    let input =
        MessageCreateInput::new(chat_id, body, BodyType::Plain, &sent_at).with_client_id(key);
    let resp = sc.message_create(&input).await.expect("Message/set create");
    created_id(&resp, key)
}

/// Oracle: RFC 8620 §2 — `apiUrl` is "The URL to use for JMAP API
/// requests", and `primaryAccounts` maps a capability to the account to
/// use; draft-atwood-jmap-chat-00 §Account-Level Capability Object —
/// `accountCapabilities["urn:ietf:params:jmap:chat"]` carries
/// `maxBodyBytes`, `maxAttachmentBytes`, `maxAttachmentsPerMessage`,
/// `supportedBodyTypes` and `supportsThreads`, and `supportedBodyTypes`
/// "MUST include `"text/plain"`".
#[tokio::test]
async fn session_discovery_yields_usable_chat_account() {
    let (jig, client, session) = connect().await;

    assert_eq!(
        session.api_url.as_str(),
        format!("http://{}/jmap", jig.addr),
        "apiUrl must name the listener the Session was fetched from"
    );
    let account_id = session
        .chat_account_id()
        .expect("primaryAccounts must name a chat account")
        .to_owned();
    let cap = session
        .chat_capability(&account_id)
        .expect("chat account capability must parse")
        .expect("chat account capability must be present");
    assert!(cap.supported_body_types.contains(&BodyType::Plain));

    // The discovered apiUrl actually serves chat methods.
    let sc = client.with_chat_session(session);
    let chats = sc.chat_get(None, None).await.expect("Chat/get via apiUrl");
    assert_eq!(chats.account_id.as_ref(), account_id);
    assert!(chats.list.is_empty(), "fresh jig has no chats");
}

/// Oracle: draft-atwood-jmap-chat-00 §Chat/set — a direct create takes
/// `contactId`, a group create takes `name` and `memberIds`; "If a direct
/// Chat with this contactId already exists, the server MUST reject the
/// create with a SetError of type `alreadyExists`, including the existing
/// Chat's id in the `existingId` field". For the group: "`memberIds`
/// (String[], required): ChatContact.ids of additional initial members."
/// and "The server [...] adds the creator to `members` with at least one
/// role granting permissions sufficient to administer the chat"
/// (§ChatMember `role`: "Either `"admin"` or `"member"`").
#[tokio::test]
async fn chat_create_direct_and_group_round_trip() {
    let (_jig, client, session) = connect().await;
    let sc = client.with_chat_session(session);
    let alice = Id::from("contact-alice");

    let direct = sc
        .chat_create(&ChatCreateInput::Direct {
            client_id: Some("d1"),
            contact_id: &alice,
        })
        .await
        .expect("Chat/set direct create");
    let direct_id = created_id(&direct, "d1");
    let group_id = create_group(&sc, "g1", "Team").await;

    let got = sc
        .chat_get(Some(&[direct_id.clone(), group_id.clone()]), None)
        .await
        .expect("Chat/get");
    let direct_chat = got
        .list
        .iter()
        .find(|c| c.id == direct_id)
        .expect("direct chat");
    assert_eq!(direct_chat.kind, ChatKind::Direct);
    assert_eq!(direct_chat.contact_id.as_ref(), Some(&alice));
    let group_chat = got
        .list
        .iter()
        .find(|c| c.id == group_id)
        .expect("group chat");
    assert_eq!(group_chat.kind, ChatKind::Group);
    assert_eq!(group_chat.name.as_deref(), Some("Team"));
    // The jig's single caller has no principal id, so the creator is the
    // account itself.
    let mut members: Vec<(&str, &str)> = group_chat
        .members
        .as_ref()
        .expect("group chat members")
        .iter()
        .map(|m| (m.id.as_ref(), m.role.as_str()))
        .collect();
    members.sort();
    let mut expected = vec![
        (got.account_id.as_ref(), "admin"),
        ("contact-bob", "member"),
    ];
    expected.sort();
    assert_eq!(members, expected);

    let dup = sc
        .chat_create(&ChatCreateInput::Direct {
            client_id: Some("d2"),
            contact_id: &alice,
        })
        .await
        .expect("Chat/set duplicate direct create");
    let err = dup
        .not_created
        .as_ref()
        .and_then(|m| m.get("d2"))
        .expect("duplicate direct chat must be rejected");
    assert_eq!(err.error_type, "alreadyExists");
    assert_eq!(err.existing_id.as_ref(), Some(&direct_id));
}

/// Oracle: draft-atwood-jmap-chat-00 §Message/set — create takes `chatId`,
/// `body`, `bodyType`, `sentAt` and optionally `replyTo`, `senderExpiresAt`,
/// `burnOnRead`; "The server sets `id`, `senderMsgId`, `senderId`,
/// `receivedAt`"; §Message/query — "Default sort: `receivedAt` ascending
/// when `chatId` is present".
#[tokio::test]
async fn message_create_query_get_round_trip() {
    let (_jig, client, session) = connect().await;
    let sc = client.with_chat_session(session);
    let chat_id = create_group(&sc, "g1", "Team").await;

    let first = send(&sc, &chat_id, "m1", "first").await;
    let second = send(&sc, &chat_id, "m2", "second").await;

    let sent_at = UTCDate::from(SENT_AT);
    let expires = UTCDate::from("2999-01-01T00:00:00Z");
    let mut reply =
        MessageCreateInput::new(&chat_id, "reply", BodyType::Plain, &sent_at).with_client_id("m3");
    reply.reply_to = Some(&first);
    reply.sender_expires_at = Some(&expires);
    reply.burn_on_read = Some(true);
    let resp = sc.message_create(&reply).await.expect("Message/set reply");
    let third = created_id(&resp, "m3");

    let mut query = MessageQueryInput::default().with_sort_ascending(true);
    query.chat_id = Some(&chat_id);
    let ids = sc.message_query(&query).await.expect("Message/query").ids;
    let mut sorted_ids = ids.clone();
    sorted_ids.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
    let mut expected = vec![first.clone(), second.clone(), third.clone()];
    expected.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
    assert_eq!(
        sorted_ids, expected,
        "query must return exactly the chat's messages"
    );

    let got = sc.message_get(&ids, None).await.expect("Message/get");
    let by_id = |id: &Id| {
        got.list
            .iter()
            .find(|m| &m.id == id)
            .expect("message in /get")
    };
    let received: Vec<&str> = ids
        .iter()
        .map(|id| by_id(id).received_at.as_ref())
        .collect();
    assert!(
        received.windows(2).all(|w| w[0] <= w[1]),
        "ascending query must order by receivedAt: {received:?}"
    );

    let m1 = by_id(&first);
    assert_eq!(m1.body, "first");
    assert_eq!(m1.body_type, "text/plain");
    assert_eq!(m1.chat_id, chat_id);
    assert_eq!(m1.sender_id, SenderId::Owner);
    let m3 = by_id(&third);
    assert_eq!(m3.reply_to.as_ref(), Some(&first));
    assert_eq!(m3.sender_expires_at.as_ref(), Some(&expires));
    assert_eq!(m3.burn_on_read, Some(true));
}

/// Oracle: draft-atwood-jmap-chat-00 §Creating a Message — "Optional:
/// `attachments` (Attachment[]), `mentions` (Mention[]), ... `actions`
/// (MessageAction[]), ... `topicId` (String), `subject` (String), ...
/// `expiresAfterReadSeconds` (UnsignedInt)."; §Input Validation —
/// "`topicId`: reject an empty value or a value exceeding 255 bytes."
/// and a failed check gets "an `invalidProperties` SetError ... whose
/// `properties` names the offending property." (bd:JMAP-yt1w.1).
#[tokio::test]
async fn message_create_keeps_optional_properties() {
    let (_jig, client, session) = connect().await;
    let sc = client.with_chat_session(session);
    let chat_id = create_group(&sc, "g1", "Team").await;

    let attachments: Vec<Attachment> = serde_json::from_value(serde_json::json!([{
        "blobId": "b1",
        "filename": "notes.txt",
        "contentType": "text/plain",
        "size": 3,
        "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    }]))
    .expect("Attachment");
    let mentions: Vec<Mention> = serde_json::from_value(serde_json::json!([
        { "id": "contact-bob", "offset": 3, "length": 4 }
    ]))
    .expect("Mention");
    let actions: Vec<MessageAction> = serde_json::from_value(serde_json::json!([
        { "type": "urn:jmap:chat:cap:vtc", "uri": "https://example.com/room" }
    ]))
    .expect("MessageAction");
    let sent_at = UTCDate::from(SENT_AT);
    let mut input = MessageCreateInput::new(&chat_id, "hi @bob", BodyType::Plain, &sent_at)
        .with_client_id("m1");
    input.attachments = Some(&attachments);
    input.mentions = Some(&mentions);
    input.actions = Some(&actions);
    input.topic_id = Some("release");
    input.subject = Some("Release 1.0");
    input.expires_after_read_seconds = Some(30);
    let resp = sc.message_create(&input).await.expect("Message/set create");
    let id = created_id(&resp, "m1");

    let got = sc.message_get(&[id], None).await.expect("Message/get");
    let msg = &got.list[0];
    assert_eq!(msg.attachments, attachments);
    assert_eq!(msg.mentions, mentions);
    assert_eq!(msg.actions, actions);
    assert_eq!(msg.topic_id.as_deref(), Some("release"));
    assert_eq!(msg.subject.as_deref(), Some("Release 1.0"));
    assert_eq!(msg.expires_after_read_seconds, Some(30));

    let mut empty_topic =
        MessageCreateInput::new(&chat_id, "hi", BodyType::Plain, &sent_at).with_client_id("m2");
    empty_topic.topic_id = Some("");
    let resp = sc
        .message_create(&empty_topic)
        .await
        .expect("Message/set create");
    let err = resp
        .not_created
        .as_ref()
        .and_then(|m| m.get("m2"))
        .unwrap_or_else(|| panic!("empty topicId must be rejected: {resp:?}"));
    assert_eq!(err.error_type, "invalidProperties");
    assert_eq!(err.properties.as_deref(), Some(&["topicId".to_owned()][..]));
}

/// Oracle: draft-atwood-jmap-chat-00 §Fields Inside the Ciphertext —
/// "Servers MUST reject a `Message/set` create or update, or a
/// `Peer/deliver` payload, that has an encrypted `bodyType` and any of
/// these fields non-empty, with `invalidArguments`."; §Rich Body Format —
/// "Both the `mentions` and `broadcastMentions` arrays on the Message
/// MUST be empty" (bd:JMAP-yt1w.1).
#[tokio::test]
async fn message_create_rejects_fields_forbidden_by_body_type() {
    let (_jig, client, session) = connect().await;
    let sc = client.with_chat_session(session);
    let chat_id = create_group(&sc, "g1", "Team").await;
    let sent_at = UTCDate::from(SENT_AT);
    let mentions: Vec<Mention> = serde_json::from_value(serde_json::json!([
        { "id": "contact-bob", "offset": 3, "length": 4 }
    ]))
    .expect("Mention");

    let encrypted = BodyType::Other("application/mls-ciphertext".to_owned());
    let mut e2ee =
        MessageCreateInput::new(&chat_id, "Y2lwaGVy", encrypted, &sent_at).with_client_id("m1");
    e2ee.topic_id = Some("release");
    let mut rich =
        MessageCreateInput::new(&chat_id, "hi @bob", BodyType::Rich, &sent_at).with_client_id("m2");
    rich.mentions = Some(&mentions);

    for (input, cid) in [(&e2ee, "m1"), (&rich, "m2")] {
        let resp = sc.message_create(input).await.expect("Message/set create");
        let err = resp
            .not_created
            .as_ref()
            .and_then(|m| m.get(cid))
            .unwrap_or_else(|| panic!("{cid} must be rejected: {resp:?}"));
        assert_eq!(err.error_type, "invalidArguments", "{cid}");
    }
}

/// Oracle: RFC 8620 §5.5 Comparator `isAscending` — "If false, reverse
/// the comparator's results to sort in descending order."
/// (bd:JMAP-yt1w.1). The sends are spaced so the millisecond `receivedAt`
/// values differ.
#[tokio::test]
async fn message_query_descending_returns_newest_first() {
    let (_jig, client, session) = connect().await;
    let sc = client.with_chat_session(session);
    let chat_id = create_group(&sc, "g1", "Team").await;

    let mut sent = Vec::new();
    for key in ["m1", "m2", "m3"] {
        sent.push(send(&sc, &chat_id, key, key).await);
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }

    let mut query = MessageQueryInput::default().with_sort_ascending(false);
    query.chat_id = Some(&chat_id);
    let ids = sc.message_query(&query).await.expect("Message/query").ids;
    sent.reverse();
    assert_eq!(ids, sent, "descending query must return newest first");
}

/// Oracle: draft-atwood-jmap-chat-00 §Adding and Removing Reactions — add
/// patches `"reactions/<senderReactionId>": {"emoji", "sentAt"}` and "The
/// server MUST set `senderId` to `"self"`"; remove patches the key to
/// `null` and "The server MUST remove the entry".
#[tokio::test]
async fn reactions_add_then_remove() {
    let (_jig, client, session) = connect().await;
    let sc = client.with_chat_session(session);
    let chat_id = create_group(&sc, "g1", "Team").await;
    let msg_id = send(&sc, &chat_id, "m1", "hello").await;
    let sent_at = UTCDate::from(SENT_AT);
    let reaction_id = "01JREACTION0000000000000001";

    let add = [ReactionChange::Add {
        sender_reaction_id: reaction_id,
        emoji: "\u{1F44D}",
        sent_at: &sent_at,
    }];
    let mut patch = MessagePatch::default();
    patch.reaction_changes = Some(&add);
    let resp = sc
        .message_update(&msg_id, &patch)
        .await
        .expect("add reaction");
    assert!(
        resp.not_updated.as_ref().is_none_or(|m| m.is_empty()),
        "{resp:?}"
    );

    let got = sc
        .message_get(std::slice::from_ref(&msg_id), None)
        .await
        .expect("Message/get");
    let reaction = got.list[0]
        .reactions
        .get(reaction_id)
        .expect("added reaction must be stored under its senderReactionId");
    assert_eq!(reaction.emoji, "\u{1F44D}");
    assert_eq!(reaction.sender_id, SenderId::Owner);

    let remove = [ReactionChange::Remove {
        sender_reaction_id: reaction_id,
    }];
    let mut patch = MessagePatch::default();
    patch.reaction_changes = Some(&remove);
    let resp = sc
        .message_update(&msg_id, &patch)
        .await
        .expect("remove reaction");
    assert!(
        resp.not_updated.as_ref().is_none_or(|m| m.is_empty()),
        "{resp:?}"
    );

    let got = sc.message_get(&[msg_id], None).await.expect("Message/get");
    assert!(
        got.list[0].reactions.is_empty(),
        "{:?}",
        got.list[0].reactions
    );
}

/// Oracle: draft-atwood-jmap-chat-00 §Editing a Message — "`update` with
/// changed `body`, `bodyType`, `mentions`, and/or `broadcastMentions`, on a
/// message where `senderId` is `"self"` and `deletedAt` is absent. The
/// server MUST reject an edit of any other message with a `forbidden`
/// SetError."
///
/// Only the deleted-message half is exercised: the jig's single caller
/// cannot author a Message whose `senderId` is not `"self"`.
#[tokio::test]
async fn message_edit_after_delete_is_forbidden() {
    let (_jig, client, session) = connect().await;
    let sc = client.with_chat_session(session);
    let chat_id = create_group(&sc, "g1", "Team").await;
    let msg_id = send(&sc, &chat_id, "m1", "hello").await;

    let deleted_at = UTCDate::from("2026-01-02T00:00:00Z");
    let mut patch = MessagePatch::default();
    patch.deleted_at = Some(&deleted_at);
    let resp = sc.message_update(&msg_id, &patch).await.expect("delete");
    assert!(
        resp.not_updated.as_ref().is_none_or(|m| m.is_empty()),
        "{resp:?}"
    );

    let mut patch = MessagePatch::default();
    patch.body = Some("rewritten");
    let resp = sc.message_update(&msg_id, &patch).await.expect("edit");
    let err = resp
        .not_updated
        .as_ref()
        .and_then(|m| m.get(&msg_id))
        .unwrap_or_else(|| panic!("edit of a deleted Message must be rejected: {resp:?}"));
    assert_eq!(err.error_type, "forbidden");

    let got = sc.message_get(&[msg_id], None).await.expect("Message/get");
    assert_eq!(got.list[0].body, "");
}

/// Oracle: draft-atwood-jmap-chat-00 §ReadPosition — `lastReadMessageId`
/// is "The `id` of the most recent Message the owner has read in this
/// Chat"; §ReadPosition/set updates it.
///
/// The record comes from the server, not the client: "For direct and
/// group Chats, a ReadPosition is created when the first message is
/// delivered."
#[tokio::test]
async fn read_position_update_round_trip() {
    let (_jig, client, session) = connect().await;
    let sc = client.with_chat_session(session.clone());
    let chat_id = create_group(&sc, "g1", "Team").await;
    let msg_id = send(&sc, &chat_id, "m1", "hello").await;

    let positions = sc.read_position_get(None).await.expect("ReadPosition/get");
    let rp = positions
        .list
        .iter()
        .find(|p| p.chat_id == chat_id)
        .expect("read position for the chat");
    let resp = sc
        .read_position_update(&rp.id, &msg_id)
        .await
        .expect("ReadPosition/set update");
    assert!(
        resp.not_updated.as_ref().is_none_or(|m| m.is_empty()),
        "{resp:?}"
    );

    let got = sc
        .read_position_get(Some(std::slice::from_ref(&rp.id)))
        .await
        .expect("ReadPosition/get");
    assert_eq!(got.list[0].last_read_message_id.as_ref(), Some(&msg_id));
}

/// Oracle: RFC 8620 §5.2 — `/changes` returns `oldState` echoing
/// `sinceState`, a `newState`, and in `created` the ids "for records that
/// have been created since the old state".
#[tokio::test]
async fn chat_changes_reports_created_chat() {
    let (_jig, client, session) = connect().await;
    let sc = client.with_chat_session(session);
    let since = sc.chat_get(None, None).await.expect("Chat/get").state;

    let chat_id = create_group(&sc, "g1", "Team").await;

    let changes = sc.chat_changes(&since, None).await.expect("Chat/changes");
    assert_eq!(changes.old_state, since);
    assert_ne!(changes.new_state, since);
    assert_eq!(changes.created, vec![chat_id]);
    assert!(changes.updated.is_empty());
    assert!(changes.destroyed.is_empty());
    assert!(!changes.has_more_changes);
}

/// Oracle: draft-atwood-jmap-chat-00 §Space/join — "The server MUST verify
/// that no active SpaceBan exists for the requesting account's owner in the
/// target Space. If a ban exists, the server MUST reject the request with a
/// `forbidden` method error."
///
/// The jig's single caller is both the Space creator and the banned user.
#[tokio::test]
async fn space_join_rejects_banned_caller() {
    let (_jig, client, session) = connect().await;
    let account_id = Id::from(session.chat_account_id().expect("chat account"));
    let sc = client.with_chat_session(session);

    let resp = sc
        .space_create(&SpaceCreateInput::new("Lobby").with_client_id("s0"))
        .await
        .expect("Space/set create");
    let space_id = created_id(&resp, "s0");
    let mut patch = SpacePatch::default();
    patch.is_public = Some(true);
    let resp = sc
        .space_update(&space_id, &patch)
        .await
        .expect("Space/set update");
    assert!(
        resp.not_updated.as_ref().is_none_or(|m| m.is_empty()),
        "{resp:?}"
    );

    let mut ban = SpaceBanCreateInput::new(&space_id, &account_id);
    ban.client_id = Some("b0");
    let resp = sc
        .space_ban_create(&ban)
        .await
        .expect("SpaceBan/set create");
    created_id(&resp, "b0");

    match sc.space_join(&SpaceJoinInput::SpaceId(&space_id)).await {
        Err(ClientError::MethodError { error_type, .. }) => assert_eq!(error_type, "forbidden"),
        other => panic!("expected forbidden method error, got {other:?}"),
    }
}
