//! The sending side of draft-atwood-jmap-chat-federation-00. Line
//! numbers cite draft-atwood-jmap-chat-federation-00.md at
//! /home/mark/PROJECT/jmap-chat-spec HEAD (85162c4); expected values are
//! hand-written from the quoted text.

mod common;

use common::*;
use jmap_chat_federation_server::{
    aggregate_delivery_state, is_transient, other_participants, outbound_reference,
    peer_group_update_args, DeliveryFailure, OutboundError, SenderIdentities,
};
use jmap_chat_server::GroupUpdateAction;
use jmap_chat_types::{Chat, ChatContact, ChatMember, DeliveryState, MessageAction, Space};
use jmap_types::Id;
use serde_json::{json, Value};

const PEER_ACCOUNT: &str = "peer-acct";

fn chat(value: Value) -> Chat {
    serde_json::from_value(value).expect("chat fixture")
}

fn member(value: Value) -> ChatMember {
    serde_json::from_value(value).expect("member fixture")
}

fn contacts() -> Vec<ChatContact> {
    [contact(BOB_ID, BOB, false), contact(CAROL_ID, CAROL, false)]
        .into_iter()
        .map(|c| serde_json::from_value(c).expect("contact fixture"))
        .collect()
}

async fn reference(fed: &Fed, local_id: Option<&str>) -> Option<Id> {
    outbound_reference(
        &fed.chat,
        &(),
        &Id::from(ACCOUNT),
        local_id.map(Id::from).as_ref(),
    )
    .await
    .expect("read")
}

// L398: "If that message was composed by the owner (`senderId` is the
// owner's `accountId`), its `senderMsgId` equals its `id` and no
// translation is needed."
#[tokio::test]
async fn owner_composed_reference_is_its_id() {
    let fed = Fed::new();
    fed.seed(
        "Message",
        stored_message("m-own", "m-own", GROUP, ACCOUNT, PAST),
    );
    assert_eq!(
        reference(&fed, Some("m-own")).await,
        Some(Id::from("m-own"))
    );
}

// L396: "the sending server MUST translate any `replyTo` or `threadRootId`
// values from locally-stored receiver-assigned `id` values to the
// corresponding `senderMsgId` values". L398: "If the message was received
// from a peer, use the stored `senderMsgId`."
#[tokio::test]
async fn received_reference_translates_to_sender_msg_id() {
    let fed = Fed::new();
    fed.seed(
        "Message",
        stored_message(
            "local-7",
            "01J00000000000000000000PEER",
            GROUP,
            BOB_ID,
            PAST,
        ),
    );
    assert_eq!(
        reference(&fed, Some("local-7")).await,
        Some(Id::from("01J00000000000000000000PEER"))
    );
}

// L398: "If no `senderMsgId` can be found for the referenced message, the
// `replyTo` field MUST be omitted from the outbound payload". L400: "If
// the thread root message cannot be resolved to a `senderMsgId`,
// `threadRootId` MUST be omitted".
#[tokio::test]
async fn unresolvable_reference_is_omitted() {
    let fed = Fed::new();
    assert_eq!(reference(&fed, Some("hard-deleted")).await, None);
    assert_eq!(reference(&fed, None).await, None);
}

// L584: "When a `Message/set` update sets `deletedForAll: true` on a
// message whose `senderId` is the owner's `accountId`, the server MUST
// send `Peer/retract` to the server of every other participant in the
// Chat."
#[test]
fn retract_goes_to_every_other_participant() {
    let account = Id::from(ACCOUNT);
    let group = chat(group_chat(&[ACCOUNT, BOB_ID, CAROL_ID], false));
    assert_eq!(
        other_participants(&group, None, &account),
        vec![Id::from(BOB_ID), Id::from(CAROL_ID)]
    );
    // draft-atwood-jmap-chat-00 L580 `contactId`: "Direct chats only. The
    // ChatContact.id of the other participant."
    let direct = chat(direct_chat(LOW, BOB_ID));
    assert_eq!(
        other_participants(&direct, None, &account),
        vec![Id::from(BOB_ID)]
    );
}

// draft-atwood-jmap-chat-00 L806-809: "A SpaceMember describes one participant in
// a Space." / "The participant's ChatContact.id, or the accountId for the
// owner."
#[test]
fn channel_participants_are_space_members() {
    let mut space_json = space(&[], false);
    space_json["members"] = json!([
        {"id": ACCOUNT, "roleIds": [], "joinedAt": PAST},
        {"id": BOB_ID, "roleIds": ["poster"], "joinedAt": PAST},
    ]);
    let space: Space = serde_json::from_value(space_json).expect("space fixture");
    let channel = chat(channel(0, json!([])));
    assert_eq!(
        other_participants(&channel, Some(&space), &Id::from(ACCOUNT)),
        vec![Id::from(BOB_ID)]
    );
}

fn group_update(chat_value: Value, action: &GroupUpdateAction) -> Result<Value, OutboundError> {
    let account = Id::from(ACCOUNT);
    let contacts = contacts();
    let ids = SenderIdentities::new(&account, OWNER, &contacts);
    peer_group_update_args(&ids, Id::from(PEER_ACCOUNT), &chat(chat_value), action)
        .map(|a| serde_json::to_value(a).expect("serialize"))
}

// L615: `senderUserId` is "The identity string of the user making the
// change". L629: `members` is the "Full membership list at the time of
// this update, including the calling server's owner. Each ChatMember `id`
// and `invitedBy` carries an identity string." L641-647: `name`,
// `description`, `avatarBlobId` "MAY be present for `"create"`".
#[test]
fn create_sends_full_membership_as_identity_strings() {
    let mut group = group_chat(&[ACCOUNT, BOB_ID], false);
    group["members"][1]["invitedBy"] = json!(ACCOUNT);
    group["description"] = json!("d");
    assert_eq!(
        group_update(group, &GroupUpdateAction::Create).expect("args"),
        json!({
            "accountId": PEER_ACCOUNT,
            "chatId": GROUP,
            "senderUserId": OWNER,
            "action": "create",
            "members": [
                {"id": OWNER, "role": "member", "joinedAt": PAST},
                {"id": BOB, "role": "member", "joinedAt": PAST, "invitedBy": OWNER},
            ],
            "name": "g",
            "description": "d",
        })
    );
}

// L631: `addedMembers` "required for `"addMembers"`"; L635:
// `removedMemberIds` "The identity strings of the members who were
// removed"; L638: `updatedRoles` entries "MUST contain `id` (String, the
// identity string of the member ...) and `role`".
#[test]
fn member_changes_carry_identity_strings() {
    let group = group_chat(&[ACCOUNT, BOB_ID, CAROL_ID], false);
    let carol =
        member(json!({"id": CAROL_ID, "role": "member", "joinedAt": PAST, "invitedBy": ACCOUNT}));
    let added = group_update(
        group.clone(),
        &GroupUpdateAction::AddMembers { added: vec![carol] },
    )
    .expect("args");
    assert_eq!(added["action"], json!("addMembers"));
    assert_eq!(
        added["addedMembers"],
        json!([{"id": CAROL, "role": "member", "joinedAt": PAST, "invitedBy": OWNER}])
    );

    let removed = group_update(
        group.clone(),
        &GroupUpdateAction::RemoveMembers {
            removed_ids: vec![Id::from(CAROL_ID)],
        },
    )
    .expect("args");
    assert_eq!(removed["action"], json!("removeMembers"));
    assert_eq!(removed["removedMemberIds"], json!([CAROL]));

    let bob_admin = member(json!({"id": BOB_ID, "role": "admin", "joinedAt": PAST}));
    let roles = group_update(
        group,
        &GroupUpdateAction::UpdateRoles {
            updated: vec![bob_admin],
        },
    )
    .expect("args");
    assert_eq!(roles["action"], json!("updateRoles"));
    assert_eq!(roles["updatedRoles"], json!([{"id": BOB, "role": "admin"}]));
}

// L625/L626: `"leave"` and `"close"` take "Arguments: none beyond the
// standard fields."
#[test]
fn leave_and_close_carry_only_standard_fields() {
    for (action, name) in [
        (GroupUpdateAction::Leave, "leave"),
        (GroupUpdateAction::Close, "close"),
    ] {
        assert_eq!(
            group_update(group_chat(&[BOB_ID], false), &action).expect("args"),
            json!({
                "accountId": PEER_ACCOUNT,
                "chatId": GROUP,
                "senderUserId": OWNER,
                "action": name,
            })
        );
    }
}

// L624: `"updateMetadata"` — "The group's `name`, `description`, or
// `avatarBlobId` was changed."
#[test]
fn update_metadata_carries_stored_metadata() {
    let mut group = group_chat(&[ACCOUNT, BOB_ID], false);
    group["avatarBlobId"] = json!("blob-9");
    let args = group_update(group, &GroupUpdateAction::UpdateMetadata).expect("args");
    assert_eq!(args["name"], json!("g"));
    assert_eq!(args["avatarBlobId"], json!("blob-9"));
    assert!(args.get("members").is_none());
}

// L635: removed members are named by "identity strings"; a local id with
// no ChatContact has none, and sending the local id would be meaningless
// to the peer.
#[test]
fn member_without_identity_is_an_error() {
    assert_eq!(
        group_update(
            group_chat(&[ACCOUNT, BOB_ID], false),
            &GroupUpdateAction::RemoveMembers {
                removed_ids: vec![Id::from("contact-gone")],
            },
        ),
        Err(OutboundError::UnknownUser(Id::from("contact-gone")))
    );
}

fn task_action(assignee_ids: Value) -> MessageAction {
    serde_json::from_value(json!({
        "type": "urn:ietf:params:jmap:chat:cap:task",
        "uri": "urn:jmap:task:A1:T1",
        "label": "Ship it",
        "metadata": {"assigneeIds": assignee_ids, "due": "2026-11-01T00:00:00Z"},
    }))
    .expect("action fixture")
}

// draft-atwood-jmap-chat-tasks-00 L254: "Between servers it carries
// identity strings, not local ids ...: the sending server maps each local
// id to the user's identity". Federation draft L314 (jmap-chat-spec
// 63e9d08): the owner's `accountId` becomes
// its `ownerUserId`, a ChatContact `id` that ChatContact's `identity`.
// Other actions are not user references and are sent unchanged.
#[test]
fn task_assignee_ids_become_identity_strings() {
    let contacts = contacts();
    let account = Id::from(ACCOUNT);
    let identities = SenderIdentities::new(&account, OWNER, &contacts);
    let other: MessageAction = serde_json::from_value(json!({
        "type": "urn:example:other",
        "uri": "https://other.example/1",
        "metadata": {"assigneeIds": [BOB_ID]},
    }))
    .expect("action fixture");
    let sent = identities
        .actions(&[task_action(json!([ACCOUNT, BOB_ID])), other.clone()])
        .expect("mapped");
    assert_eq!(sent, [task_action(json!([OWNER, BOB])), other]);
}

// L312: "Peer/* arguments therefore never carry a ChatContact `id`": an
// assignee with no identity string cannot be sent.
#[test]
fn task_assignee_without_identity_is_an_error() {
    let contacts = contacts();
    let account = Id::from(ACCOUNT);
    let identities = SenderIdentities::new(&account, OWNER, &contacts);
    assert_eq!(
        identities.actions(&[task_action(json!(["contact-gone"]))]),
        Err(OutboundError::UnknownUser(Id::from("contact-gone")))
    );
}

// L792: "In group chats, `deliveryState` is the aggregate across
// recipients: the best state any recipient has reached, ordered
// `"pending"` < `"delivered"` < `"received"`. The aggregate is `"failed"`
// only when every recipient has permanently failed. A permanent failure
// for some recipients never downgrades the aggregate".
#[test]
fn delivery_state_is_best_recipient_state() {
    use DeliveryState::{Delivered, Failed, Pending, Received};
    let cases: [(&[DeliveryState], DeliveryState); 7] = [
        (&[Pending, Pending], Pending),
        (&[Pending, Delivered], Delivered),
        (&[Delivered, Received, Pending], Received),
        (&[Failed, Delivered], Delivered),
        (&[Failed, Pending], Pending),
        (&[Failed, Failed], Failed),
        // L787: `"pending"` — "Initial state."
        (&[], Pending),
    ];
    for (states, expected) in cases {
        assert_eq!(aggregate_delivery_state(states), expected, "{states:?}");
    }
}

// L802: "`invalidArguments` (permanent — do not retry), `forbidden`
// (permanent — do not retry), `rateLimited` (transient — retry after
// backoff), `serverFail` (transient — retry with exponential backoff).
// HTTP-level 5xx responses or timeouts are treated as transient failures."
// L775: "A delivery attempt is considered failed when the remote server
// returns a transient error ..., when the connection cannot be
// established, or when no response is received within an
// implementation-defined timeout."
#[test]
fn retry_classifier_follows_error_handling() {
    assert!(!is_transient(DeliveryFailure::MethodError(
        "invalidArguments"
    )));
    assert!(!is_transient(DeliveryFailure::MethodError("forbidden")));
    assert!(is_transient(DeliveryFailure::MethodError("rateLimited")));
    assert!(is_transient(DeliveryFailure::MethodError("serverFail")));
    assert!(is_transient(DeliveryFailure::HttpStatus(500)));
    assert!(is_transient(DeliveryFailure::HttpStatus(503)));
    assert!(is_transient(DeliveryFailure::HttpStatus(599)));
    assert!(!is_transient(DeliveryFailure::HttpStatus(400)));
    assert!(is_transient(DeliveryFailure::NoResponse));
}
