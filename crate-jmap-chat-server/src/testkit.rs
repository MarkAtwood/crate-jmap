//! Backend conformance suite for [`ChatBackend`] implementations.
//!
//! Gated behind `feature = "testkit"`. Each test is a `pub async fn`
//! generic over [`ConformanceBackend`], drives the backend through this
//! crate's method handlers, and states its oracle (draft or RFC text) in
//! its rustdoc. A backend crate runs the whole suite with one
//! [`ConformanceBackend`] impl and one macro line in an integration test:
//!
//! ```rust,ignore
//! // tests/backend_conformance.rs
//! jmap_chat_server::chat_backend_conformance!(MyBackend::new_for_test);
//! ```
//!
//! [`chat_backend_conformance!`](crate::chat_backend_conformance) emits
//! one `#[test]` per suite test, each calling the given constructor for a
//! fresh, empty backend and running the test through [`run`], so the
//! calling crate needs no tokio macros.
//!
//! # What the suite assumes
//!
//! Every test runs in one account, `a1`, which it registers with
//! [`ConformanceBackend::add_account`]; a few register further accounts.
//! Requests act as the owner of the account
//! ([`ConformanceBackend::caller`]). Fixtures are hand-written JSON
//! stored with [`ConformanceBackend::seed`].
//!
//! # Capability-gated tests
//!
//! A test whose assertion holds only for some backend configurations
//! returns early, and so passes, when the backend does not have that
//! configuration:
//!
//! - The `replyCount` / `unreadReplyCount` tests check
//!   [`ChatLimits::supports_threads`](crate::ChatLimits::supports_threads)
//!   from [`ChatBackend::limits`]; one test runs only when it is `false`,
//!   the others only when it is `true`. Run the macro once per
//!   configuration to cover both.
//! - The exact `Chat.unreadCount` tests run only when
//!   [`ChatBackend::maintains_unread_count`] is `false`, where this crate
//!   keeps the count exact. draft-atwood-jmap-chat-00 §Chat `unreadCount`
//!   lets a backend that keeps the count itself update it "some time after
//!   the change that affects it" and "report a server-defined maximum".
//! - The outbound-event test runs only when
//!   [`ConformanceBackend::take_outbound_events`] returns `Some`.

use std::future::Future;

use jmap_types::Id;
use serde_json::{Value, json};

use crate::{ChatBackend, OutboundEvent};

/// The account every suite test runs in.
const ACCOUNT_ID: &str = "a1";

/// The `ownerUserId` the suite registers its accounts with.
const OWNER_USER_ID: &str = "owner@example.org";

/// The test seam a [`ChatBackend`] implements to run the conformance
/// suite.
///
/// The methods stand in for what a deployment does outside the JMAP
/// methods: provisioning an account, authenticating its owner, and
/// storing objects that arrived by federation or were created
/// server-side.
pub trait ConformanceBackend: ChatBackend {
    /// The caller context of a request made by the owner of
    /// `account_id`.
    ///
    /// The [`ChatBackend::create_object`] authorisation contract applies:
    /// the backend must let this caller act as the owner of `account_id`.
    fn caller(&self, account_id: &Id) -> Self::CallerCtx;

    /// Provision `account_id`, owned by `owner_user_id` (the `ownerUserId`
    /// Session field, as returned by [`ChatBackend::owner_user_id`]), so
    /// [`JmapBackend::account_exists`](jmap_server::JmapBackend::account_exists)
    /// reports it and it holds no objects.
    fn add_account(&self, account_id: &Id, owner_user_id: &str) -> impl Future<Output = ()> + Send;

    /// Store `object`, the wire JSON of a `type_name` object (for example
    /// `"Message"`), in `account_id` exactly as given, under the id in its
    /// `id` member, replacing any object stored under that id.
    ///
    /// Fixtures use this for objects no client method creates (received
    /// Messages, ChatContacts, PresenceStatus) and for fields no client
    /// may set. The suite reads state before and after the writes it
    /// tests, so whether seeding advances the state does not matter.
    /// Objects may omit properties that have a default.
    fn seed(
        &self,
        account_id: &Id,
        type_name: &'static str,
        object: Value,
    ) -> impl Future<Output = ()> + Send;

    /// Drain the [`OutboundEvent`]s the backend has enqueued since the
    /// last call, oldest first.
    ///
    /// Return `None` when the backend cannot observe its own outbound
    /// queue; the tests that check events then pass without running.
    fn take_outbound_events(&self) -> Option<Vec<OutboundEvent>>;
}

/// Run `test` to completion on a fresh multi-thread tokio runtime with
/// four workers, so the concurrency tests race on real threads.
///
/// # Panics
///
/// If the runtime cannot be built, and whenever `test` panics.
pub fn run<F: Future>(test: F) -> F::Output {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .build()
        .expect("build the tokio runtime")
        .block_on(test)
}

/// Emit one `#[test]` per conformance test, each running it on a fresh
/// backend from `$make`.
///
/// `$make` is any expression callable with no arguments that returns a
/// [`ConformanceBackend`](crate::testkit::ConformanceBackend), such as a
/// constructor path or a closure. Invoke it at most once per module.
///
/// ```rust,ignore
/// jmap_chat_server::chat_backend_conformance!(MyBackend::new_for_test);
/// ```
#[macro_export]
macro_rules! chat_backend_conformance {
    (@tests ($make:expr) $($name:ident),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                $crate::testkit::run($crate::testkit::$name(($make)()));
            }
        )*
    };
    ($make:expr $(,)?) => {
        $crate::chat_backend_conformance!(@tests ($make)
            state_advances_after_create,
            create_then_get,
            is_contact_blocked_reads_chat_contact_blocked_field,
            is_contact_blocked_reads_only_the_owner_list,
            path_key_updates_nested_member,
            top_level_object_value_replaces_property,
            path_through_absent_parent_is_invalid_patch,
            prefix_conflict_is_invalid_patch,
            path_depth_counts_toward_nesting_cap,
            chat_muted_null_resets_to_false,
            chat_pinned_message_ids_null_resets_to_empty,
            chat_receive_typing_indicators_null_resets_to_true,
            chat_receipt_sharing_null_removes_override,
            contact_blocked_null_resets_to_false,
            presence_null_resets_to_online,
            presence_receipt_sharing_null_resets_to_true,
            message_mentions_null_resets_to_empty,
            message_actions_null_resets_to_empty,
            message_reactions_null_resets_to_empty,
            message_broadcast_mentions_null_resets_to_empty,
            chat_query_filters_kind_and_muted,
            chat_query_applies_filter_operators,
            contact_query_filters_blocked_and_presence,
            message_query_filters_topic_id,
            message_query_filters_has_attachment_both_ways,
            message_query_has_mention_false_excludes_mentions,
            message_query_changes_refuses_text_filter,
            message_query_changes_requires_chat_id_or_has_mention,
            space_query_filters_name_substring,
            emoji_query_filters_space_id,
            emoji_query_changes_applies_space_id_filter,
            space_query_sort_descending_reverses_order,
            space_query_sort_ascending_orders_by_property,
            contact_query_sort_last_active_at_absent_sorts_last,
            anchor_with_positive_offset_starts_after_anchor,
            anchor_with_negative_offset_starts_before_anchor,
            anchor_without_offset_starts_at_anchor,
            anchor_offset_before_start_clamps_to_zero,
            anchor_absent_from_results_is_anchor_not_found,
            anchor_overrides_position,
            anchor_offset_without_anchor_is_ignored,
            anchor_arguments_of_wrong_type_are_invalid_arguments,
            emoji_query_wrongly_typed_args_do_not_echo_value,
            extreme_position_and_limit_saturate,
            largest_position_is_empty_page_not_error,
            join_adds_member_and_counts_use_together,
            exhausted_invite_is_refused_without_any_write,
            concurrent_redemptions_increment_exactly_once,
            invalid_read_at_patch_is_rejected_without_burning,
            create_message_stores_and_reports_one_new_message,
            contact_create_by_identity_assigns_id,
            contact_create_existing_identity_already_exists,
            contact_create_without_identity_invalid_properties,
            contact_create_other_properties_invalid_properties,
            contact_query_identity_filter_matches_octet_for_octet,
            reply_counts_match_fixture,
            unread_reply_count_without_read_position,
            counts_absent_without_supports_threads,
            reply_create_lists_parent_updated,
            reply_destroy_lists_parent_updated,
            read_position_update_lists_thread_root_updated,
            sender_expires_at_hard_deletes_at_deadline,
            message_expiry_seconds_deletes_messages_older_than_policy,
            expires_after_read_seconds_counts_from_read_at,
            read_burn_on_read_message_is_deleted,
            read_rules_spare_the_senders_own_copy,
            mute_until_clears_muted_and_mute_until,
            presence_expires_at_resets_status,
            expiry_records_chat_unread_count_change,
            absent_last_read_counts_every_message,
            compliance_11_1_unread_count_from_read_position,
            compliance_11_1_with_assigned_ids,
            unread_count_in_full_get,
            read_position_set_updates_last_read_at,
            client_last_read_at_is_rejected,
            chat_destroy_destroys_its_read_position,
            space_join_creates_read_positions_for_its_channels,
            remove_channels_destroys_their_read_positions,
            space_destroy_destroys_channel_read_positions,
            read_position_set_advances_chat_state,
            message_destroy_advances_chat_state,
        );
    };
}

pub use backend_basics::*;
pub use contact_identity::*;
pub use expiry::*;
pub use message_atomic::*;
pub use patch_null::*;
pub use patch_paths::*;
pub use query_anchor::*;
pub use query_filters::*;
pub use query_sort::*;
pub use read_position::*;
pub use space_join::*;
pub use threads::*;

// ---------------------------------------------------------------------------
// Shared helpers and fixture builders
// ---------------------------------------------------------------------------

/// The caller acting as the owner of [`ACCOUNT_ID`].
fn owner<B: ConformanceBackend>(backend: &B) -> B::CallerCtx {
    backend.caller(&Id::from(ACCOUNT_ID))
}

/// Provision [`ACCOUNT_ID`].
async fn setup<B: ConformanceBackend>(backend: &B) {
    backend
        .add_account(&Id::from(ACCOUNT_ID), OWNER_USER_ID)
        .await;
}

/// Store `object` in [`ACCOUNT_ID`] as given.
async fn seed<B: ConformanceBackend>(backend: &B, type_name: &'static str, object: Value) {
    backend.seed(&Id::from(ACCOUNT_ID), type_name, object).await;
}

/// Whether the backend reports `supportsThreads` for [`ACCOUNT_ID`].
fn supports_threads<B: ConformanceBackend>(backend: &B) -> bool {
    let account_id = Id::from(ACCOUNT_ID);
    backend
        .limits(&owner(backend), &account_id)
        .supports_threads
}

/// Whether this crate keeps `Chat.unreadCount` exact for the backend
/// (see the module docs).
fn exact_unread_count<B: ConformanceBackend>(backend: &B) -> bool {
    !backend.maintains_unread_count()
}

/// Store a group Chat with the given `id` in [`ACCOUNT_ID`], with the
/// account as its admin member, so a `Message/set` create naming that
/// `chatId` passes the draft-atwood-jmap-chat-00 §Input Validation
/// `chatId` check. Overwrites any Chat already stored under `id`.
async fn seed_group_chat<B: ConformanceBackend>(backend: &B, id: &str) {
    seed(
        backend,
        "Chat",
        json!({
            "id": id,
            "kind": "group",
            "name": "G",
            "createdAt": "2026-01-01T00:00:00Z",
            "unreadCount": 0,
            "pinnedMessageIds": [],
            "members": [{ "id": ACCOUNT_ID, "role": "admin", "joinedAt": "2026-01-01T00:00:00Z" }],
        }),
    )
    .await;
}

/// Re-store Message `id` with `senderId` set to `"bob"`, so it is a copy
/// received from another participant. draft-atwood-jmap-chat-00
/// §Message `burnOnRead`: "Applies only to the receiving mailbox; the
/// sender's own copy is not affected."
async fn make_received<B: ConformanceBackend>(backend: &B, id: &str) {
    let jmap_server::HandlerOutput { primary: get, .. } = crate::message::handle_message_get(
        backend,
        &owner(backend),
        json!({ "accountId": ACCOUNT_ID, "ids": [id] }),
        &[],
    )
    .await
    .expect("Message/get");
    let mut msg = get["list"][0].clone();
    assert!(msg.is_object(), "message {id} not found: {get}");
    msg["senderId"] = json!("bob");
    seed(backend, "Message", msg).await;
}

/// Create a Chat with `kind: "group"` and the given name in
/// [`ACCOUNT_ID`] through `Chat/set` and return its server-assigned id.
async fn make_chat_group<B: ConformanceBackend>(backend: &B, name: &str) -> String {
    let jmap_server::HandlerOutput { primary: resp, .. } = crate::handle_chat_set(
        backend,
        &owner(backend),
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "c0": { "kind": "group", "name": name } }
        }),
        &[],
    )
    .await
    .expect("make_chat_group: handle_chat_set");
    resp["created"]["c0"]["id"]
        .as_str()
        .expect("make_chat_group: server-assigned id")
        .to_owned()
}

/// Create a Space with the given name in [`ACCOUNT_ID`] through
/// `Space/set` and return its server-assigned id.
async fn make_space<B: ConformanceBackend>(backend: &B, name: &str) -> String {
    let jmap_server::HandlerOutput { primary: resp, .. } = crate::handle_space_set(
        backend,
        &owner(backend),
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "s0": { "name": name } }
        }),
        &[],
    )
    .await
    .expect("make_space: handle_space_set");
    resp["created"]["s0"]["id"]
        .as_str()
        .expect("make_space: server-assigned id")
        .to_owned()
}

// ---------------------------------------------------------------------------
// Backend basics: state, create, get, is_contact_blocked
// ---------------------------------------------------------------------------

mod backend_basics {
    use jmap_chat_types::{Chat, ChatKind};
    use jmap_types::{Id, UTCDate};
    use serde_json::json;

    use super::{ACCOUNT_ID, ConformanceBackend, OWNER_USER_ID, seed, setup};

    /// Oracle: state advances after a successful create_object call.
    pub async fn state_advances_after_create<B: ConformanceBackend>(backend: B) {
        let account_id = Id::from("account1");
        backend.add_account(&account_id, OWNER_USER_ID).await;
        let caller = backend.caller(&account_id);

        let state_before = backend
            .get_state::<Chat>(&caller, &account_id)
            .await
            .expect("get_state");

        let chat = |id: &Id| {
            Chat::new(
                id.clone(),
                ChatKind::Group,
                UTCDate::from("2026-01-01T00:00:00Z"),
                0,
                vec![],
                false,
                true,
            )
        };
        backend
            .create_object::<Chat, _>(&caller, &account_id, "c0", chat)
            .await
            .expect("create_object");

        let state_after = backend
            .get_state::<Chat>(&caller, &account_id)
            .await
            .expect("get_state");
        assert_ne!(
            state_before, state_after,
            "state must change after create_object"
        );
    }

    /// Oracle: created objects are retrievable by id.
    pub async fn create_then_get<B: ConformanceBackend>(backend: B) {
        let account_id = Id::from("account1");
        backend.add_account(&account_id, OWNER_USER_ID).await;
        let caller = backend.caller(&account_id);

        let chat = |id: &Id| {
            Chat::new(
                id.clone(),
                ChatKind::Group,
                UTCDate::from("2026-01-01T00:00:00Z"),
                0,
                vec![],
                false,
                true,
            )
        };
        let (server_id, _) = backend
            .create_object::<Chat, _>(&caller, &account_id, "c0", chat)
            .await
            .expect("create_object");

        let (found, not_found) = backend
            .get_objects::<Chat>(
                &caller,
                &account_id,
                Some(std::slice::from_ref(&server_id)),
                None,
            )
            .await
            .expect("get_objects");

        assert_eq!(found.len(), 1);
        assert!(not_found.is_empty());
        assert_eq!(found[0].id, server_id);
    }

    /// Oracle: `is_contact_blocked` reads `ChatContact.blocked` from the
    /// store. The backend's predicate is exercised directly, mirroring the
    /// way a consumer's transport layer would consult it before a fan-out.
    pub async fn is_contact_blocked_reads_chat_contact_blocked_field<B: ConformanceBackend>(
        backend: B,
    ) {
        setup(&backend).await;
        let account_id = Id::from(ACCOUNT_ID);
        let caller = backend.caller(&account_id);
        let blocked_id = Id::from("blocked-user");
        let allowed_id = Id::from("allowed-user");

        seed(
            &backend,
            "ChatContact",
            json!({
                "id": "blocked-user",
                "identity": "b@example.org",
                "login": "b@example.org",
                "firstSeenAt": "2024-01-01T00:00:00Z",
                "lastSeenAt": "2024-01-02T00:00:00Z",
                "blocked": true
            }),
        )
        .await;
        seed(
            &backend,
            "ChatContact",
            json!({
                "id": "allowed-user",
                "identity": "a@example.org",
                "login": "a@example.org",
                "firstSeenAt": "2024-01-01T00:00:00Z",
                "lastSeenAt": "2024-01-02T00:00:00Z",
                "blocked": false
            }),
        )
        .await;

        assert!(
            backend
                .is_contact_blocked(&caller, &account_id, &blocked_id)
                .await
                .expect("is_contact_blocked"),
            "blocked: true contact must report blocked"
        );
        assert!(
            !backend
                .is_contact_blocked(&caller, &account_id, &allowed_id)
                .await
                .expect("is_contact_blocked"),
            "blocked: false contact must report not-blocked"
        );

        // Unknown contact id → not blocked (open-by-default for an
        // unrecognised principal).
        let unknown = Id::from("never-seen");
        assert!(
            !backend
                .is_contact_blocked(&caller, &account_id, &unknown)
                .await
                .expect("is_contact_blocked"),
            "unknown contact id must report not-blocked"
        );
    }

    /// Oracle: draft-atwood-jmap-chat-00 §Blocked Contacts (L1698): "Typing and
    /// presence push events whose sending or referenced ChatContact has
    /// `blocked: true` on the receiving owner's contact list are similarly
    /// dropped", and §ChatContact `id` (L426): "It identifies the ChatContact
    /// only on this server". Two accounts each hold a ChatContact for the
    /// other; only the recipient's own list decides, and a contact id is never
    /// resolved in another account's list.
    pub async fn is_contact_blocked_reads_only_the_owner_list<B: ConformanceBackend>(backend: B) {
        let recipient = Id::from("acct-recipient");
        let sender = Id::from("acct-sender");
        backend.add_account(&recipient, OWNER_USER_ID).await;
        backend.add_account(&sender, "sender@example.org").await;

        backend
            .seed(
                &recipient,
                "ChatContact",
                json!({
                    "id": "c-sender",
                    "identity": "sender@example.org",
                    "login": "sender@example.org",
                    "firstSeenAt": "2024-01-01T00:00:00Z",
                    "lastSeenAt": "2024-01-02T00:00:00Z",
                    "blocked": true
                }),
            )
            .await;
        backend
            .seed(
                &sender,
                "ChatContact",
                json!({
                    "id": "c-recipient",
                    "identity": "recipient@example.org",
                    "login": "recipient@example.org",
                    "firstSeenAt": "2024-01-01T00:00:00Z",
                    "lastSeenAt": "2024-01-02T00:00:00Z",
                    "blocked": false
                }),
            )
            .await;

        let blocked = |account: &Id, contact: &str| {
            let backend = &backend;
            let account = account.clone();
            let contact = Id::from(contact);
            async move {
                backend
                    .is_contact_blocked(&backend.caller(&account), &account, &contact)
                    .await
                    .expect("is_contact_blocked")
            }
        };

        assert!(
            blocked(&recipient, "c-sender").await,
            "the recipient blocked the sender on the recipient's own list"
        );
        assert!(
            !blocked(&sender, "c-recipient").await,
            "the sender's list does not block the recipient"
        );
        assert!(
            !blocked(&sender, "c-sender").await,
            "c-sender is held only by the recipient's account"
        );
    }
}

// ---------------------------------------------------------------------------
// `/set update` PatchObject paths (RFC 8620 §5.3, bd:JMAP-cr0z)
//
// These tests drive the backend's `update_object` against the Message
// `deliveryReceipts` map, a real object-of-objects property.
// draft-atwood-jmap-chat-00 §Message marks it "`deliveryReceipts`
// (Object, optional, server-set)", so `Message/set` rejects client
// patches to it; the server's own delivery-receipt writes reach the
// backend through this path.
// ---------------------------------------------------------------------------

mod patch_paths {
    use jmap_chat_types::Message;
    use jmap_server::HandlerOutput;
    use jmap_server::{BackendSetError, SetErrorType};
    use jmap_types::{Id, PatchObject};
    use serde_json::{Value, json};

    use super::{ACCOUNT_ID, ConformanceBackend, owner, seed_group_chat, setup};
    use crate::message::{handle_message_get, handle_message_set};

    /// Create a Message and return its server id.
    async fn create_message<B: ConformanceBackend>(backend: &B) -> String {
        seed_group_chat(backend, "c1").await;
        let HandlerOutput { primary: resp, .. } = handle_message_set(
            backend,
            &owner(backend),
            &std::collections::BTreeMap::new(),
            json!({
                "accountId": ACCOUNT_ID,
                "create": { "m0": { "chatId": "c1", "body": "hi", "sentAt": "2024-01-01T00:00:00Z" } }
            }),
            &[],
        )
        .await
        .expect("create");
        resp["created"]["m0"]["id"]
            .as_str()
            .expect("created id")
            .to_owned()
    }

    /// Apply `patch` to message `id` in the backend, returning the SetError
    /// type on rejection.
    async fn update<B: ConformanceBackend>(
        backend: &B,
        id: &str,
        patch: Value,
    ) -> Result<(), SetErrorType> {
        let patch: PatchObject = serde_json::from_value(patch).expect("patch is a JSON object");
        match backend
            .update_object::<Message>(&owner(backend), &Id::from(ACCOUNT_ID), &Id::from(id), patch)
            .await
        {
            Ok(_) => Ok(()),
            Err(BackendSetError::SetError(e)) => Err(e.error_type),
            Err(e) => panic!("backend error: {e:?}"),
        }
    }

    /// Fetch message `id` as its wire JSON.
    async fn get<B: ConformanceBackend>(backend: &B, id: &str) -> Value {
        let HandlerOutput { primary: resp, .. } = handle_message_get(
            backend,
            &owner(backend),
            json!({ "accountId": ACCOUNT_ID, "ids": [id] }),
            &[],
        )
        .await
        .expect("get");
        resp["list"][0].clone()
    }

    /// Create a Message whose `deliveryReceipts` holds a single entry `u1`.
    async fn message_with_receipt<B: ConformanceBackend>(backend: &B) -> String {
        let id = create_message(backend).await;
        let resp = update(
            backend,
            &id,
            json!({ "deliveryReceipts": { "u1": { "deliveredAt": "2024-01-01T00:00:01Z" } } }),
        )
        .await;
        assert_eq!(resp, Ok(()));
        id
    }

    /// Oracle: RFC 8620 §5.3: "The keys are a path in JSON Pointer format
    /// \[RFC6901\], with an implicit leading "/"". The key
    /// `deliveryReceipts/u2` therefore adds member `u2` of the
    /// `deliveryReceipts` object, keeping `u1`, rather than creating a
    /// property literally named `deliveryReceipts/u2`.
    pub async fn path_key_updates_nested_member<B: ConformanceBackend>(backend: B) {
        setup(&backend).await;
        let id = message_with_receipt(&backend).await;

        let resp = update(
            &backend,
            &id,
            json!({ "deliveryReceipts/u2": { "readAt": "2024-01-01T00:00:02Z" } }),
        )
        .await;
        assert_eq!(resp, Ok(()));

        let msg = get(&backend, &id).await;
        assert!(msg.get("deliveryReceipts/u2").is_none(), "{msg:?}");
        assert_eq!(
            msg["deliveryReceipts"],
            json!({
                "u1": { "deliveredAt": "2024-01-01T00:00:01Z" },
                "u2": { "readAt": "2024-01-01T00:00:02Z" },
            })
        );
    }

    /// Oracle: RFC 8620 §5.3: "Anything else: The value to set for this
    /// property (this may be a replacement or addition to the object being
    /// patched)." A top-level `deliveryReceipts` value replaces the whole
    /// property; the existing `u1` entry is not merged into it.
    pub async fn top_level_object_value_replaces_property<B: ConformanceBackend>(backend: B) {
        setup(&backend).await;
        let id = message_with_receipt(&backend).await;

        let resp = update(
            &backend,
            &id,
            json!({ "deliveryReceipts": { "u2": { "readAt": "2024-01-01T00:00:02Z" } } }),
        )
        .await;
        assert_eq!(resp, Ok(()));

        let msg = get(&backend, &id).await;
        assert_eq!(
            msg["deliveryReceipts"],
            json!({ "u2": { "readAt": "2024-01-01T00:00:02Z" } })
        );
    }

    /// Oracle: RFC 8620 §5.3: "All parts prior to the last (i.e., the value
    /// after the final slash) MUST already exist on the object being
    /// patched." and "if there is any violation, the update MUST be
    /// rejected with an "invalidPatch" error". A fresh Message has no
    /// `deliveryReceipts`, so `deliveryReceipts/u1` is rejected and the
    /// stored Message is unchanged.
    pub async fn path_through_absent_parent_is_invalid_patch<B: ConformanceBackend>(backend: B) {
        setup(&backend).await;
        let id = create_message(&backend).await;
        let before = get(&backend, &id).await;
        assert!(before.get("deliveryReceipts").is_none(), "{before:?}");

        let resp = update(
            &backend,
            &id,
            json!({ "deliveryReceipts/u1": { "readAt": "2024-01-01T00:00:02Z" } }),
        )
        .await;
        assert_eq!(resp, Err(SetErrorType::InvalidPatch));
        assert_eq!(get(&backend, &id).await, before);
    }

    /// Oracle: RFC 8620 §5.3: "There MUST NOT be two patches in the
    /// PatchObject where the pointer of one is the prefix of the pointer of
    /// the other, e.g., "alerts/1/offset" and "alerts"." and "if there is
    /// any violation, the update MUST be rejected with an "invalidPatch"
    /// error". The stored Message is unchanged.
    pub async fn prefix_conflict_is_invalid_patch<B: ConformanceBackend>(backend: B) {
        setup(&backend).await;
        let id = message_with_receipt(&backend).await;
        let before = get(&backend, &id).await;

        let resp = update(
            &backend,
            &id,
            json!({
                "deliveryReceipts": { "u2": { "readAt": "2024-01-01T00:00:02Z" } },
                "deliveryReceipts/u1/readAt": "2024-01-01T00:00:03Z",
            }),
        )
        .await;
        assert_eq!(resp, Err(SetErrorType::InvalidPatch));
        assert_eq!(get(&backend, &id).await, before);
    }

    /// `{"k": {"k": ... {}}}` with `levels` object levels.
    fn nested(levels: usize) -> Value {
        (1..levels).fold(json!({}), |inner, _| json!({ "k": inner }))
    }

    /// Oracle: bd:JMAP-wlip.1 caps `/set update` nesting at 32 levels and
    /// rejects deeper patches as RFC 8620 §5.3 "invalidPatch", leaving the
    /// record unchanged. Because §5.3 makes keys "a path in JSON Pointer
    /// format", the path itself reaches into the object, so its segments
    /// count toward the cap. Test vector: a 21-segment path into an
    /// existing 21-level value, setting another 21-level value, would store
    /// 43 levels; each repeat could add 21 more.
    pub async fn path_depth_counts_toward_nesting_cap<B: ConformanceBackend>(backend: B) {
        setup(&backend).await;
        let id = create_message(&backend).await;
        let resp = update(
            &backend,
            &id,
            json!({ "deliveryReceipts": { "u1": nested(21) } }),
        )
        .await;
        assert_eq!(resp, Ok(()));
        let before = get(&backend, &id).await;

        let path = format!("deliveryReceipts/u1{}", "/k".repeat(19));
        let resp = update(&backend, &id, json!({ path: nested(21) })).await;
        assert_eq!(resp, Err(SetErrorType::InvalidPatch));
        assert_eq!(get(&backend, &id).await, before);
    }
}

// ---------------------------------------------------------------------------
// `/set update` with a `null` PatchObject value resets a property to its
// spec default (RFC 8620 §5.3, bd:JMAP-sofk).
//
// RFC 8620 §5.3: "null: The value to set for this property is the default
// value if specified for this property". A backend that removes the
// member on `null` must deserialize each property below to its
// draft-atwood-jmap-chat-00 default when absent; otherwise the following
// `/get` fails as a whole.
// ---------------------------------------------------------------------------

mod patch_null {
    use jmap_chat_types::Message;
    use jmap_server::HandlerOutput;
    use jmap_types::{Id, PatchObject};
    use serde_json::{Map, Value, json};

    use super::{
        ACCOUNT_ID, ConformanceBackend, make_chat_group, owner, seed, seed_group_chat, setup,
    };
    use crate::message::{handle_message_get, handle_message_set};
    use crate::{
        handle_chat_get, handle_chat_set, handle_contact_get, handle_contact_set,
        handle_presence_get, handle_presence_set,
    };

    /// Assert a `/set` response updated `id` without error.
    fn assert_updated(resp: &Value, id: &str) {
        assert_eq!(resp["notUpdated"], Value::Null, "{resp:?}");
        assert!(
            resp["updated"]
                .as_object()
                .is_some_and(|m| m.contains_key(id)),
            "{resp:?}"
        );
    }

    async fn chat_update<B: ConformanceBackend>(backend: &B, id: &str, patch: Value) {
        let HandlerOutput { primary: resp, .. } = handle_chat_set(
            backend,
            &owner(backend),
            &std::collections::BTreeMap::new(),
            json!({ "accountId": ACCOUNT_ID, "update": { id: patch } }),
            &[],
        )
        .await
        .expect("Chat/set");
        assert_updated(&resp, id);
    }

    async fn chat_get<B: ConformanceBackend>(backend: &B, id: &str) -> Value {
        let HandlerOutput { primary: resp, .. } = handle_chat_get(
            backend,
            &owner(backend),
            json!({ "accountId": ACCOUNT_ID, "ids": [id] }),
            &[],
        )
        .await
        .expect("Chat/get");
        resp["list"][0].clone()
    }

    /// Oracle: draft-atwood-jmap-chat-00 Chat `muted`: "When `true`, push
    /// notifications for this chat are suppressed. \[...\] Default: `false`."
    pub async fn chat_muted_null_resets_to_false<B: ConformanceBackend>(backend: B) {
        setup(&backend).await;
        let id = make_chat_group(&backend, "g").await;
        chat_update(&backend, &id, json!({ "muted": true })).await;

        chat_update(&backend, &id, json!({ "muted": null })).await;

        assert_eq!(chat_get(&backend, &id).await["muted"], json!(false));
    }

    /// Oracle: draft-atwood-jmap-chat-00 Chat `pinnedMessageIds`: "Ordered
    /// list of pinned Message ids, most-recently-pinned first. \[...\] Empty by
    /// default."
    pub async fn chat_pinned_message_ids_null_resets_to_empty<B: ConformanceBackend>(backend: B) {
        setup(&backend).await;
        let id = make_chat_group(&backend, "g").await;

        chat_update(&backend, &id, json!({ "pinnedMessageIds": null })).await;

        assert_eq!(chat_get(&backend, &id).await["pinnedMessageIds"], json!([]));
    }

    /// Oracle: draft-atwood-jmap-chat-00 Chat `receiveTypingIndicators`:
    /// "Default: `true`."
    pub async fn chat_receive_typing_indicators_null_resets_to_true<B: ConformanceBackend>(
        backend: B,
    ) {
        setup(&backend).await;
        let id = make_chat_group(&backend, "g").await;
        chat_update(&backend, &id, json!({ "receiveTypingIndicators": false })).await;

        chat_update(&backend, &id, json!({ "receiveTypingIndicators": null })).await;

        assert_eq!(
            chat_get(&backend, &id).await["receiveTypingIndicators"],
            json!(true)
        );
    }

    /// Oracle: draft-atwood-jmap-chat-00 Chat `receiptSharing` (line 645-646
    /// at spec 29e9a41): "(Boolean, optional) Per-Chat override of the
    /// account-level `PresenceStatus.receiptSharing` preference ... When
    /// absent, the account-level preference applies." The property has no
    /// default, so `null` removes the override and `/get` omits it
    /// (bd:JMAP-w9ox.2).
    pub async fn chat_receipt_sharing_null_removes_override<B: ConformanceBackend>(backend: B) {
        setup(&backend).await;
        let id = make_chat_group(&backend, "g").await;
        chat_update(&backend, &id, json!({ "receiptSharing": false })).await;
        assert_eq!(
            chat_get(&backend, &id).await["receiptSharing"],
            json!(false)
        );

        chat_update(&backend, &id, json!({ "receiptSharing": null })).await;

        let chat = chat_get(&backend, &id).await;
        assert!(
            chat.as_object()
                .is_some_and(|m| !m.contains_key("receiptSharing")),
            "{chat:?}"
        );
    }

    /// Oracle: draft-atwood-jmap-chat-00 ChatContact `blocked`: "When
    /// `true`, messages from this ChatContact are silently dropped by this
    /// mailbox \[...\] Default: `false`."
    pub async fn contact_blocked_null_resets_to_false<B: ConformanceBackend>(backend: B) {
        setup(&backend).await;
        // ChatContact objects are server-managed (create is forbidden).
        seed(
            &backend,
            "ChatContact",
            json!({
                "id": "ct1",
                "identity": "alice@example.com",
                "login": "alice@example.com",
                "firstSeenAt": "2024-01-01T00:00:00Z",
                "lastSeenAt": "2024-01-01T00:00:00Z",
                "blocked": true,
            }),
        )
        .await;

        let HandlerOutput { primary: resp, .. } = handle_contact_set(
            &backend,
            &owner(&backend),
            json!({ "accountId": ACCOUNT_ID, "update": { "ct1": { "blocked": null } } }),
        )
        .await
        .expect("ChatContact/set");
        assert_updated(&resp, "ct1");

        let HandlerOutput { primary: resp, .. } = handle_contact_get(
            &backend,
            &owner(&backend),
            json!({ "accountId": ACCOUNT_ID, "ids": ["ct1"] }),
        )
        .await
        .expect("ChatContact/get");
        assert_eq!(resp["list"][0]["blocked"], json!(false), "{resp:?}");
    }

    /// Seed a PresenceStatus with non-default `presence` and `receiptSharing`.
    async fn seed_presence<B: ConformanceBackend>(backend: &B) {
        // PresenceStatus objects are server-managed (only update is permitted).
        seed(
            backend,
            "PresenceStatus",
            json!({
                "id": "ps1",
                "presence": "busy",
                "receiptSharing": false,
                "updatedAt": "2024-01-01T00:00:00Z",
            }),
        )
        .await;
    }

    async fn presence_null_then_get<B: ConformanceBackend>(backend: &B, prop: &str) -> Value {
        let HandlerOutput { primary: resp, .. } = handle_presence_set(
            backend,
            &owner(backend),
            json!({ "accountId": ACCOUNT_ID, "update": { "ps1": { prop: null } } }),
        )
        .await
        .expect("PresenceStatus/set");
        assert_updated(&resp, "ps1");

        let HandlerOutput { primary: resp, .. } = handle_presence_get(
            backend,
            &owner(backend),
            json!({ "accountId": ACCOUNT_ID, "ids": ["ps1"] }),
        )
        .await
        .expect("PresenceStatus/get");
        resp["list"][0].clone()
    }

    /// Oracle: draft-atwood-jmap-chat-00 PresenceStatus `presence`: "One of
    /// `"online"`, `"away"`, `"busy"`, `"invisible"`, or `"offline"`.
    /// Default is `"online"`."
    pub async fn presence_null_resets_to_online<B: ConformanceBackend>(backend: B) {
        setup(&backend).await;
        seed_presence(&backend).await;

        let ps = presence_null_then_get(&backend, "presence").await;

        assert_eq!(ps["presence"], json!("online"), "{ps:?}");
    }

    /// Oracle: draft-atwood-jmap-chat-00 PresenceStatus `receiptSharing`:
    /// "When `false`, this account does not send read receipts to peers.
    /// \[...\] Default: `true`."
    pub async fn presence_receipt_sharing_null_resets_to_true<B: ConformanceBackend>(backend: B) {
        setup(&backend).await;
        seed_presence(&backend).await;

        let ps = presence_null_then_get(&backend, "receiptSharing").await;

        assert_eq!(ps["receiptSharing"], json!(true), "{ps:?}");
    }

    /// Create a Message, patch `prop` to null in the backend, and return the
    /// fetched Message.
    ///
    /// The patch goes to `update_object` directly rather than through
    /// `Message/set`: draft-atwood-jmap-chat-00 §Message/set admits only the
    /// Editing, Reactions (`reactions/<senderReactionId>`), Deleting and
    /// Marking as Read updates, so the handler rejects a whole-property
    /// `actions` or `reactions` patch. The default-on-absence contract under
    /// test belongs to the stored record, not to the handler.
    async fn message_null_then_get<B: ConformanceBackend>(backend: &B, prop: &str) -> Value {
        setup(backend).await;
        seed_group_chat(backend, "c1").await;
        let HandlerOutput { primary: resp, .. } = handle_message_set(
            backend,
            &owner(backend),
            &std::collections::BTreeMap::new(),
            json!({
                "accountId": ACCOUNT_ID,
                "create": { "m0": { "chatId": "c1", "body": "hi", "sentAt": "2024-01-01T00:00:00Z" } }
            }),
            &[],
        )
        .await
        .expect("Message/set create");
        let id = resp["created"]["m0"]["id"]
            .as_str()
            .expect("created id")
            .to_owned();

        let patch = PatchObject::from(Map::from_iter([(prop.to_owned(), Value::Null)]));
        backend
            .update_object::<Message>(
                &owner(backend),
                &Id::from(ACCOUNT_ID),
                &Id::from(id.as_str()),
                patch,
            )
            .await
            .expect("update_object");

        let HandlerOutput { primary: resp, .. } = handle_message_get(
            backend,
            &owner(backend),
            json!({ "accountId": ACCOUNT_ID, "ids": [&id] }),
            &[],
        )
        .await
        .expect("Message/get");
        resp["list"][0].clone()
    }

    /// Oracle: draft-atwood-jmap-chat-00 Message `mentions`: "Structured
    /// per-user @mention annotations. Empty by default."
    pub async fn message_mentions_null_resets_to_empty<B: ConformanceBackend>(backend: B) {
        let msg = message_null_then_get(&backend, "mentions").await;
        assert_eq!(msg["mentions"], json!([]), "{msg:?}");
    }

    /// Oracle: draft-atwood-jmap-chat-00 Message `actions`: "Out-of-band
    /// action invitations associated with this message. Empty by default."
    pub async fn message_actions_null_resets_to_empty<B: ConformanceBackend>(backend: B) {
        let msg = message_null_then_get(&backend, "actions").await;
        assert_eq!(msg["actions"], json!([]), "{msg:?}");
    }

    /// Oracle: draft-atwood-jmap-chat-00 Message `reactions`: "Emoji
    /// reactions, keyed by `senderReactionId`. \[...\] Empty object by
    /// default."
    pub async fn message_reactions_null_resets_to_empty<B: ConformanceBackend>(backend: B) {
        let msg = message_null_then_get(&backend, "reactions").await;
        assert_eq!(msg["reactions"], json!({}), "{msg:?}");
    }

    /// Oracle: draft-atwood-jmap-chat-00 Message `broadcastMentions`:
    /// "Structured broadcast-scope mention annotations \[...\]. Empty by
    /// default."
    pub async fn message_broadcast_mentions_null_resets_to_empty<B: ConformanceBackend>(
        backend: B,
    ) {
        let msg = message_null_then_get(&backend, "broadcastMentions").await;
        assert_eq!(msg["broadcastMentions"], json!([]), "{msg:?}");
    }
}

// ---------------------------------------------------------------------------
// `/query` filters (bd:JMAP-vwr2)
//
// The backend applies every `/query` filter clause the draft defines,
// FilterOperators included, and `/queryChanges` applies the same filter.
//
// Oracles: draft-atwood-jmap-chat-00 (jmap-chat-spec 33702a5):
// - L1064 §ChatContact/query: "Filter properties: `blocked` (Boolean),
//   `presence` (String), `identity` (String; ...)."
// - L1139 §Chat/query: "Filter properties: `kind` (String), `muted`
//   (Boolean)."
// - L1279 §Message/query `text`: "Servers that do not support full-text
//   search MUST return `unsupportedFilter`."
// - L1285 `topicId`: "Return only messages whose `topicId` equals this
//   value."
// - L1288 `hasAttachment`: "Filter to messages with or without
//   attachments."
// - L1291 `hasMention`: "Filter to messages that mention the owner: the
//   accountId appears in `mentions`, OR ..."
// - L1389 §Space/query: "Filter properties: `name` (String, substring
//   match), `isPublic` (Boolean)."
// - L1482 §CustomEmoji/query: "Filter properties: `spaceId` (String,
//   optional — if absent returns all emoji accessible to the account
//   including server-global)."
// - RFC 8620 §5.5 FilterOperator: "AND: All of the conditions must match
//   for the filter to match. OR: At least one of the conditions must
//   match for the filter to match. NOT: None of the conditions must match
//   for the filter to match."
// - RFC 8620 §5.6: `/queryChanges` takes the "filter" "as given in the
//   query", and `added` lists every id "added to the results since the
//   old state".
//
// Objects are seeded as hand-written JSON.
// ---------------------------------------------------------------------------

mod query_filters {
    use jmap_server::HandlerOutput;
    use jmap_types::JmapError;
    use serde_json::{Value, json};

    use super::{ACCOUNT_ID, ConformanceBackend, owner, seed, setup};
    use crate::message::{handle_message_query, handle_message_query_changes};
    use crate::{
        handle_chat_query, handle_contact_query, handle_emoji_query, handle_emoji_query_changes,
        handle_emoji_set, handle_space_query,
    };

    fn sorted_ids(resp: &Value) -> Vec<String> {
        let mut ids: Vec<String> = resp["ids"]
            .as_array()
            .expect("ids")
            .iter()
            .map(|id| id.as_str().expect("id").to_owned())
            .collect();
        ids.sort();
        ids
    }

    async fn seeded<B: ConformanceBackend>(
        backend: &B,
        type_name: &'static str,
        objects: &[(&str, Value)],
    ) {
        setup(backend).await;
        for (id, obj) in objects {
            let mut obj = obj.clone();
            obj["id"] = json!(id);
            seed(backend, type_name, obj).await;
        }
    }

    async fn chat_backend<B: ConformanceBackend>(backend: &B) {
        seeded(
            backend,
            "Chat",
            &[
                ("g-muted", json!({ "kind": "group", "muted": true })),
                ("g-loud", json!({ "kind": "group", "muted": false })),
                ("d-loud", json!({ "kind": "direct", "muted": false })),
            ],
        )
        .await;
    }

    async fn chat_query<B: ConformanceBackend>(backend: &B, filter: Value) -> Vec<String> {
        let HandlerOutput { primary: resp, .. } = handle_chat_query(
            backend,
            &owner(backend),
            json!({ "accountId": ACCOUNT_ID, "filter": filter }),
            &[],
        )
        .await
        .expect("Chat/query");
        sorted_ids(&resp)
    }

    /// L1139 (jmap-chat-spec 33702a5) §Chat/query: "Filter properties:
    /// `kind` (String), `muted` (Boolean)."
    ///
    /// L1139: `kind` and `muted` each restrict the result; together both must
    /// hold.
    pub async fn chat_query_filters_kind_and_muted<B: ConformanceBackend>(backend: B) {
        chat_backend(&backend).await;
        assert_eq!(
            chat_query(&backend, json!({ "kind": "group" })).await,
            ["g-loud", "g-muted"]
        );
        assert_eq!(
            chat_query(&backend, json!({ "muted": true })).await,
            ["g-muted"]
        );
        assert_eq!(
            chat_query(&backend, json!({ "kind": "group", "muted": false })).await,
            ["g-loud"]
        );
    }

    /// RFC 8620 §5.5 FilterOperator: "AND: All of the conditions must match
    /// for the filter to match. OR: At least one of the conditions must
    /// match for the filter to match. NOT: None of the conditions must match
    /// for the filter to match."
    ///
    /// RFC 8620 §5.5 OR and NOT over §Chat/query conditions.
    pub async fn chat_query_applies_filter_operators<B: ConformanceBackend>(backend: B) {
        chat_backend(&backend).await;
        let or = json!({ "operator": "OR", "conditions": [
            { "kind": "direct" }, { "muted": true },
        ] });
        assert_eq!(chat_query(&backend, or).await, ["d-loud", "g-muted"]);
        let not = json!({ "operator": "NOT", "conditions": [{ "kind": "direct" }] });
        assert_eq!(chat_query(&backend, not).await, ["g-loud", "g-muted"]);
        let and = json!({ "operator": "AND", "conditions": [
            { "kind": "group" }, { "muted": false },
        ] });
        assert_eq!(chat_query(&backend, and).await, ["g-loud"]);
    }

    /// L1064 (jmap-chat-spec 33702a5) §ChatContact/query: "Filter
    /// properties: `blocked` (Boolean), `presence` (String), `identity`
    /// (String; ...)."
    ///
    /// L1064: `blocked` and `presence` each restrict `ChatContact/query`.
    pub async fn contact_query_filters_blocked_and_presence<B: ConformanceBackend>(backend: B) {
        seeded(
            &backend,
            "ChatContact",
            &[
                (
                    "blocked-online",
                    json!({ "identity": "a", "blocked": true, "presence": "online" }),
                ),
                (
                    "open-online",
                    json!({ "identity": "b", "blocked": false, "presence": "online" }),
                ),
                (
                    "open-away",
                    json!({ "identity": "c", "blocked": false, "presence": "away" }),
                ),
            ],
        )
        .await;
        let query = |filter: Value| {
            let backend = &backend;
            async move {
                let HandlerOutput { primary: resp, .. } = handle_contact_query(
                    backend,
                    &owner(backend),
                    json!({ "accountId": ACCOUNT_ID, "filter": filter }),
                )
                .await
                .expect("ChatContact/query");
                sorted_ids(&resp)
            }
        };
        assert_eq!(
            query(json!({ "blocked": false })).await,
            ["open-away", "open-online"]
        );
        assert_eq!(
            query(json!({ "presence": "online" })).await,
            ["blocked-online", "open-online"]
        );
        assert_eq!(
            query(json!({ "blocked": false, "presence": "online" })).await,
            ["open-online"]
        );
    }

    async fn message_backend<B: ConformanceBackend>(backend: &B) {
        let msg = |extra: Value| {
            let mut m = json!({ "chatId": "chat1", "receivedAt": "2026-01-01T00:00:00Z" });
            for (k, v) in extra.as_object().expect("object") {
                m[k] = v.clone();
            }
            m
        };
        seeded(
            backend,
            "Message",
            &[
                (
                    "topic-a",
                    msg(json!({ "topicId": "a", "subject": "Topic A" })),
                ),
                (
                    "topic-b",
                    msg(json!({ "topicId": "b", "subject": "Topic B" })),
                ),
                (
                    "with-file",
                    msg(json!({ "attachments": [{ "blobId": "b1", "type": "text/plain" }] })),
                ),
                (
                    "mentions-me",
                    msg(json!({ "mentions": [{ "id": ACCOUNT_ID }] })),
                ),
            ],
        )
        .await;
        seed(backend, "Chat", json!({ "id": "chat1", "kind": "group" })).await;
    }

    async fn message_query<B: ConformanceBackend>(backend: &B, extra: Value) -> Vec<String> {
        let mut filter = json!({ "chatId": "chat1" });
        for (k, v) in extra.as_object().expect("object") {
            filter[k] = v.clone();
        }
        let HandlerOutput { primary: resp, .. } = handle_message_query(
            backend,
            &owner(backend),
            json!({ "accountId": ACCOUNT_ID, "filter": filter }),
            &[],
        )
        .await
        .expect("Message/query");
        sorted_ids(&resp)
    }

    /// L1285 (jmap-chat-spec 33702a5) `topicId`: "Return only messages
    /// whose `topicId` equals this value."
    ///
    /// L1285: `topicId` matches equal values only.
    pub async fn message_query_filters_topic_id<B: ConformanceBackend>(backend: B) {
        message_backend(&backend).await;
        assert_eq!(
            message_query(&backend, json!({ "topicId": "a" })).await,
            ["topic-a"]
        );
    }

    /// L1288 (jmap-chat-spec 33702a5) `hasAttachment`: "Filter to messages
    /// with or without attachments."
    ///
    /// L1288: `hasAttachment` selects messages "with or without attachments".
    pub async fn message_query_filters_has_attachment_both_ways<B: ConformanceBackend>(backend: B) {
        message_backend(&backend).await;
        assert_eq!(
            message_query(&backend, json!({ "hasAttachment": true })).await,
            ["with-file"]
        );
        assert_eq!(
            message_query(&backend, json!({ "hasAttachment": false })).await,
            ["mentions-me", "topic-a", "topic-b"]
        );
    }

    /// L1291 (jmap-chat-spec 33702a5) `hasMention`: "Filter to messages that
    /// mention the owner: the accountId appears in `mentions`, OR ..."
    ///
    /// L1291: `hasMention` is a Boolean filter, so `false` selects the
    /// messages that do not mention the owner.
    pub async fn message_query_has_mention_false_excludes_mentions<B: ConformanceBackend>(
        backend: B,
    ) {
        message_backend(&backend).await;
        assert_eq!(
            message_query(&backend, json!({ "hasMention": false })).await,
            ["topic-a", "topic-b", "with-file"]
        );
    }

    /// L1279 (jmap-chat-spec 33702a5) §Message/query `text`: "Servers that
    /// do not support full-text search MUST return `unsupportedFilter`."
    ///
    /// L1279 and RFC 8620 §5.6 (the filter "as given in the query"): a
    /// `text` filter the server cannot run is `unsupportedFilter` on
    /// `Message/queryChanges` as on `Message/query`.
    pub async fn message_query_changes_refuses_text_filter<B: ConformanceBackend>(backend: B) {
        message_backend(&backend).await;
        let err = handle_message_query_changes(
            &backend,
            &owner(&backend),
            json!({
                "accountId": ACCOUNT_ID,
                "filter": { "chatId": "chat1", "text": "hello" },
                "sinceQueryState": "0",
            }),
            &[],
        )
        .await
        .expect_err("text filter");
        assert_eq!(err, JmapError::unsupported_filter());
    }

    /// L1269: "Servers MUST return an `unsupportedFilter` error for any
    /// request that omits `chatId` without also including `hasMention:
    /// true`" applies to `Message/queryChanges` too.
    pub async fn message_query_changes_requires_chat_id_or_has_mention<B: ConformanceBackend>(
        backend: B,
    ) {
        message_backend(&backend).await;
        let err = handle_message_query_changes(
            &backend,
            &owner(&backend),
            json!({
                "accountId": ACCOUNT_ID,
                "filter": { "topicId": "a" },
                "sinceQueryState": "0",
            }),
            &[],
        )
        .await
        .expect_err("no chatId");
        assert_eq!(err, JmapError::unsupported_filter());
    }

    /// L1389 (jmap-chat-spec 33702a5) §Space/query: "Filter properties:
    /// `name` (String, substring match), `isPublic` (Boolean)."
    ///
    /// L1389: `name` is a substring match.
    pub async fn space_query_filters_name_substring<B: ConformanceBackend>(backend: B) {
        let member = json!([{ "id": ACCOUNT_ID }]);
        seeded(
            &backend,
            "Space",
            &[
                ("rust", json!({ "name": "rustaceans", "members": member })),
                ("go", json!({ "name": "gophers", "members": member })),
            ],
        )
        .await;
        let HandlerOutput { primary: resp, .. } = handle_space_query(
            &backend,
            &owner(&backend),
            json!({ "accountId": ACCOUNT_ID, "filter": { "name": "stace" } }),
            &[],
        )
        .await
        .expect("Space/query");
        assert_eq!(sorted_ids(&resp), ["rust"]);
    }

    async fn emoji_set<B: ConformanceBackend>(backend: &B, create: Value) -> Value {
        handle_emoji_set(
            backend,
            &owner(backend),
            &std::collections::BTreeMap::new(),
            json!({ "accountId": ACCOUNT_ID, "create": create }),
        )
        .await
        .expect("CustomEmoji/set")
        .primary
    }

    /// L1482 (jmap-chat-spec 33702a5) §CustomEmoji/query: "Filter
    /// properties: `spaceId` (String, optional — if absent returns all
    /// emoji accessible to the account including server-global)."
    ///
    /// L1482: `spaceId` restricts the result to that Space's emoji; absent,
    /// server-global emoji are included.
    pub async fn emoji_query_filters_space_id<B: ConformanceBackend>(backend: B) {
        setup(&backend).await;
        let resp = emoji_set(
            &backend,
            json!({
                "global": { "name": "wave", "blobId": "b1" },
                "local": { "name": "crab", "blobId": "b2", "spaceId": "space1" },
            }),
        )
        .await;
        let local = resp["created"]["local"]["id"].as_str().expect("id");
        let global = resp["created"]["global"]["id"].as_str().expect("id");

        let query = |filter: Value| {
            let backend = &backend;
            async move {
                let HandlerOutput { primary: resp, .. } = handle_emoji_query(
                    backend,
                    &owner(backend),
                    json!({ "accountId": ACCOUNT_ID, "filter": filter }),
                )
                .await
                .expect("CustomEmoji/query");
                sorted_ids(&resp)
            }
        };
        assert_eq!(query(json!({ "spaceId": "space1" })).await, [local]);
        let mut all = vec![local.to_owned(), global.to_owned()];
        all.sort();
        assert_eq!(query(json!({})).await, all);
    }

    /// RFC 8620 §5.6: `CustomEmoji/queryChanges` with the query's `spaceId`
    /// filter adds only emoji that the query matches.
    pub async fn emoji_query_changes_applies_space_id_filter<B: ConformanceBackend>(backend: B) {
        setup(&backend).await;
        let filter = json!({ "spaceId": "space1" });
        let HandlerOutput {
            primary: before, ..
        } = handle_emoji_query(
            &backend,
            &owner(&backend),
            json!({ "accountId": ACCOUNT_ID, "filter": filter }),
        )
        .await
        .expect("CustomEmoji/query");

        let resp = emoji_set(
            &backend,
            json!({
                "global": { "name": "wave", "blobId": "b1" },
                "local": { "name": "crab", "blobId": "b2", "spaceId": "space1" },
            }),
        )
        .await;
        let local = resp["created"]["local"]["id"].as_str().expect("id");

        let HandlerOutput {
            primary: changes, ..
        } = handle_emoji_query_changes(
            &backend,
            &owner(&backend),
            json!({
                "accountId": ACCOUNT_ID,
                "filter": filter,
                "sinceQueryState": before["queryState"],
            }),
        )
        .await
        .expect("CustomEmoji/queryChanges");
        assert_eq!(changes["added"], json!([{ "id": local, "index": 0 }]));
    }
}

// ---------------------------------------------------------------------------
// `/query` honours the request's `sort` (bd:JMAP-yt1w.1).
//
// Oracles: draft-atwood-jmap-chat-00 §Space/query "Standard JMAP
// `/query`." and §ChatContact/query "Sort properties: `lastSeenAt`,
// `login`, `lastActiveAt`. When sorting by `lastActiveAt`, ChatContacts
// for which the field is absent sort last."; RFC 8620 §5.5 Comparator
// `isAscending`: "If true, sort in ascending order. If false, reverse the
// comparator's results to sort in descending order."
// ---------------------------------------------------------------------------

mod query_sort {
    use jmap_server::HandlerOutput;
    use serde_json::{Value, json};

    use super::{ACCOUNT_ID, ConformanceBackend, make_space, owner, seed, setup};
    use crate::{handle_contact_query, handle_space_get, handle_space_query};

    /// `ids` of a `/query` response.
    fn ids(resp: &Value) -> Vec<String> {
        resp["ids"]
            .as_array()
            .expect("ids")
            .iter()
            .map(|id| id.as_str().expect("id").to_owned())
            .collect()
    }

    /// Names of the Spaces in `space_ids`, in that order.
    async fn space_names<B: ConformanceBackend>(backend: &B, space_ids: &[String]) -> Vec<String> {
        let HandlerOutput { primary: resp, .. } = handle_space_get(
            backend,
            &owner(backend),
            json!({ "accountId": ACCOUNT_ID, "ids": space_ids }),
            &[],
        )
        .await
        .expect("Space/get");
        space_ids
            .iter()
            .map(|id| {
                resp["list"]
                    .as_array()
                    .expect("list")
                    .iter()
                    .find(|s| s["id"] == id.as_str())
                    .and_then(|s| s["name"].as_str())
                    .expect("space name")
                    .to_owned()
            })
            .collect()
    }

    /// `Space/query` sorted by `name` with the given direction.
    async fn query_spaces_by_name<B: ConformanceBackend>(
        backend: &B,
        is_ascending: bool,
    ) -> Vec<String> {
        let HandlerOutput { primary: resp, .. } = handle_space_query(
            backend,
            &owner(backend),
            json!({
                "accountId": ACCOUNT_ID,
                "sort": [{ "property": "name", "isAscending": is_ascending }],
            }),
            &[],
        )
        .await
        .expect("Space/query");
        space_names(backend, &ids(&resp)).await
    }

    /// RFC 8620 §5.5 Comparator `isAscending`: "If false, reverse the
    /// comparator's results to sort in descending order."
    pub async fn space_query_sort_descending_reverses_order<B: ConformanceBackend>(backend: B) {
        setup(&backend).await;
        for name in ["Alpha", "Bravo", "Charlie"] {
            make_space(&backend, name).await;
        }
        assert_eq!(
            query_spaces_by_name(&backend, false).await,
            ["Charlie", "Bravo", "Alpha"]
        );
    }

    /// RFC 8620 §5.5 Comparator `isAscending`: "If true, sort in ascending
    /// order." The Spaces are created out of name order so creation order
    /// cannot satisfy the assertion.
    pub async fn space_query_sort_ascending_orders_by_property<B: ConformanceBackend>(backend: B) {
        setup(&backend).await;
        for name in ["Charlie", "Alpha", "Bravo"] {
            make_space(&backend, name).await;
        }
        assert_eq!(
            query_spaces_by_name(&backend, true).await,
            ["Alpha", "Bravo", "Charlie"]
        );
    }

    /// draft-atwood-jmap-chat-00 §ChatContact/query: "When sorting by
    /// `lastActiveAt`, ChatContacts for which the field is absent sort
    /// last." in both directions.
    pub async fn contact_query_sort_last_active_at_absent_sorts_last<B: ConformanceBackend>(
        backend: B,
    ) {
        setup(&backend).await;
        for (id, last_active_at) in [
            ("c1", None),
            ("c2", Some("2026-01-01T00:00:00Z")),
            ("c3", Some("2026-03-01T00:00:00Z")),
        ] {
            let mut contact = json!({ "id": id, "login": id });
            if let Some(at) = last_active_at {
                contact["lastActiveAt"] = json!(at);
            }
            seed(&backend, "ChatContact", contact).await;
        }

        for (is_ascending, expected) in [(true, ["c2", "c3", "c1"]), (false, ["c3", "c2", "c1"])] {
            let HandlerOutput { primary: resp, .. } = handle_contact_query(
                &backend,
                &owner(&backend),
                json!({
                    "accountId": ACCOUNT_ID,
                    "sort": [{ "property": "lastActiveAt", "isAscending": is_ascending }],
                }),
            )
            .await
            .expect("ChatContact/query");
            assert_eq!(ids(&resp), expected, "isAscending: {is_ascending}");
        }
    }
}

// ---------------------------------------------------------------------------
// `/query` honours `anchor` and `anchorOffset` (bd:JMAP-yt1w.22) and
// saturates extreme `position` / `limit` values (bd:JMAP-uqhn.2).
//
// Exercised through `Chat/query`, which delegates to the generic
// `jmap_server::handle_query`. With no `sort` the results are in id
// order, so five Chats `c1`..`c5` sit at indexes 0..4.
//
// Oracle: RFC 8620 §5.5 (https://www.rfc-editor.org/rfc/rfc8620.txt,
// Section 5.5 "/query").
// ---------------------------------------------------------------------------

mod query_anchor {
    use jmap_server::HandlerOutput;
    use serde_json::{Value, json};

    use super::{ACCOUNT_ID, ConformanceBackend, owner, seed, setup};
    use crate::{handle_chat_query, handle_emoji_query};

    /// Store Chats `c1`..`c5` in account `a1`.
    async fn seeded_backend<B: ConformanceBackend>(backend: &B) {
        setup(backend).await;
        for id in ["c1", "c2", "c3", "c4", "c5"] {
            seed(backend, "Chat", json!({ "id": id })).await;
        }
    }

    /// Run `Chat/query` with `extra` merged into `{ "accountId": ACCOUNT_ID }`.
    async fn query<B: ConformanceBackend>(backend: &B, extra: Value) -> Result<Value, Value> {
        let mut args = json!({ "accountId": ACCOUNT_ID });
        for (k, v) in extra.as_object().expect("extra must be an object") {
            args[k] = v.clone();
        }
        handle_chat_query(backend, &owner(backend), args, &[])
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
    pub async fn anchor_with_positive_offset_starts_after_anchor<B: ConformanceBackend>(
        backend: B,
    ) {
        seeded_backend(&backend).await;
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
    pub async fn anchor_with_negative_offset_starts_before_anchor<B: ConformanceBackend>(
        backend: B,
    ) {
        seeded_backend(&backend).await;
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
    pub async fn anchor_without_offset_starts_at_anchor<B: ConformanceBackend>(backend: B) {
        seeded_backend(&backend).await;
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
    pub async fn anchor_offset_before_start_clamps_to_zero<B: ConformanceBackend>(backend: B) {
        seeded_backend(&backend).await;
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
    pub async fn anchor_absent_from_results_is_anchor_not_found<B: ConformanceBackend>(backend: B) {
        seeded_backend(&backend).await;
        let err = query(&backend, json!({ "anchor": "nope" }))
            .await
            .expect_err("an anchor absent from the results must be rejected");
        assert_eq!(err["type"], "anchorNotFound");
    }

    /// RFC 8620 §5.5: "If an "anchor" is specified, any position argument
    /// supplied by the client MUST be ignored."
    pub async fn anchor_overrides_position<B: ConformanceBackend>(backend: B) {
        seeded_backend(&backend).await;
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
    pub async fn anchor_offset_without_anchor_is_ignored<B: ConformanceBackend>(backend: B) {
        seeded_backend(&backend).await;
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
    pub async fn anchor_arguments_of_wrong_type_are_invalid_arguments<B: ConformanceBackend>(
        backend: B,
    ) {
        seeded_backend(&backend).await;
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
    pub async fn emoji_query_wrongly_typed_args_do_not_echo_value<B: ConformanceBackend>(
        backend: B,
    ) {
        seeded_backend(&backend).await;
        for name in ["limit", "position"] {
            let mut args = json!({ "accountId": ACCOUNT_ID });
            args[name] = json!({ "note": "CANARY-zegc" });
            let err = handle_emoji_query(&backend, &owner(&backend), args)
                .await
                .expect_err("wrongly typed argument must be rejected");
            assert_eq!(err.error_type.to_string(), "invalidArguments", "{name}");
            let desc = err.description.as_deref().unwrap_or_default();
            assert!(desc.starts_with(name), "{desc}");
            assert!(desc.contains("got object"), "{desc}");
            assert!(!desc.contains("CANARY"), "{desc}");
        }
    }

    /// RFC 8620 §1.3 bounds `Int` to "-2^53+1 <= value <= 2^53-1" and
    /// `UnsignedInt` to "0 <= value <= 2^53-1". §5.5 position: "the negative
    /// value MUST be added to the total number of results given the filter,
    /// and if still negative, it's clamped to "0"", so the most negative
    /// legal `position` starts at index 0, and the largest legal `limit`
    /// returns all five results (bd:JMAP-uqhn.2).
    pub async fn extreme_position_and_limit_saturate<B: ConformanceBackend>(backend: B) {
        seeded_backend(&backend).await;
        let resp = query(
            &backend,
            json!({ "position": -9_007_199_254_740_991_i64, "limit": 9_007_199_254_740_991_u64 }),
        )
        .await
        .expect("Chat/query");
        assert_eq!(ids(&resp), ["c1", "c2", "c3", "c4", "c5"]);
        assert_eq!(resp["position"], 0);
    }

    /// RFC 8620 §5.5 position: "If the index is greater than or equal to the
    /// total number of objects in the results list, then the "ids" array in
    /// the response will be empty, but this is not an error."
    pub async fn largest_position_is_empty_page_not_error<B: ConformanceBackend>(backend: B) {
        seeded_backend(&backend).await;
        let resp = query(&backend, json!({ "position": 9_007_199_254_740_991_i64 }))
            .await
            .expect("Chat/query");
        assert!(ids(&resp).is_empty());
    }
}

// ---------------------------------------------------------------------------
// `Space/join` by invite code redeems the invite atomically through
// `ChatBackend::join_space_with_invite` (bd:JMAP-x2gd.2).
//
// Oracle: draft-atwood-jmap-chat-00 §Space/join (jmap-chat-spec 77c3474
// L1416): "The `uses` increment and membership insertion SHOULD be
// performed within a single atomic operation so that concurrent
// redemptions cannot exceed `maxUses`." This crate requires the
// recommended atomicity of every backend by choice.
//
// The joining member is always the account's owner, so two distinct
// joiners racing for the last use cannot be modelled here. These tests
// pin the properties that can be: both effects land together, the
// backend re-checks `maxUses` itself, a refusal writes nothing, and
// concurrent redemptions neither lose nor double the increment.
// ---------------------------------------------------------------------------

mod space_join {
    use std::sync::Arc;

    use jmap_chat_types::{Space, SpaceInvite};
    use jmap_types::{Id, UTCDate};
    use serde_json::json;

    use super::{ACCOUNT_ID, ConformanceBackend, owner, seed, setup};
    use crate::{InviteJoinRefusal, handle_space_join};

    const SPACE_ID: &str = "s1";
    const INVITE_ID: &str = "inv1";
    const CODE: &str = "0123456789abcdef0123456789abcdef";

    /// An empty Space and one invite for it at `uses`.
    async fn seeded<B: ConformanceBackend>(backend: &B, uses: u64, max_uses: u64) {
        setup(backend).await;
        seed(
            backend,
            "Space",
            json!({
                "id": SPACE_ID,
                "name": "Test Space",
                "createdAt": "2026-01-01T00:00:00Z",
                "memberCount": 0,
                "categories": [],
                "uncategorizedChannelIds": [],
                "isPublic": false,
                "isPubliclyPreviewable": false,
                "roles": [],
                "members": [],
            }),
        )
        .await;
        seed(
            backend,
            "SpaceInvite",
            json!({
                "id": INVITE_ID,
                "code": CODE,
                "spaceId": SPACE_ID,
                "createdBy": ACCOUNT_ID,
                "uses": uses,
                "maxUses": max_uses,
                "createdAt": "2026-01-01T00:00:00Z",
            }),
        )
        .await;
    }

    /// `(member ids, invite uses, Space state, SpaceInvite state)`.
    async fn snapshot<B: ConformanceBackend>(backend: &B) -> (Vec<String>, u64, String, String) {
        let acct = Id::from(ACCOUNT_ID);
        let (spaces, _) = backend
            .get_objects::<Space>(&owner(backend), &acct, Some(&[Id::from(SPACE_ID)]), None)
            .await
            .unwrap();
        let (invites, _) = backend
            .get_objects::<SpaceInvite>(&owner(backend), &acct, Some(&[Id::from(INVITE_ID)]), None)
            .await
            .unwrap();
        let members = spaces[0]
            .members
            .iter()
            .map(|m| m.id.as_ref().to_owned())
            .collect();
        let space_state = backend
            .get_state::<Space>(&owner(backend), &acct)
            .await
            .unwrap();
        let invite_state = backend
            .get_state::<SpaceInvite>(&owner(backend), &acct)
            .await
            .unwrap();
        (
            members,
            invites[0].uses,
            space_state.as_ref().to_owned(),
            invite_state.as_ref().to_owned(),
        )
    }

    /// draft-atwood-jmap-chat-00 §Space/join: "... atomically increment
    /// `uses` and add the caller to the Space's member list": one join
    /// produces both effects, each recorded as a change.
    pub async fn join_adds_member_and_counts_use_together<B: ConformanceBackend>(backend: B) {
        seeded(&backend, 0, 2).await;
        let (_, _, space_before, invite_before) = snapshot(&backend).await;

        handle_space_join(
            &backend,
            &owner(&backend),
            json!({ "accountId": ACCOUNT_ID, "inviteCode": CODE }),
        )
        .await
        .expect("Space/join");

        let (members, uses, space_after, invite_after) = snapshot(&backend).await;
        assert_eq!(members, vec![ACCOUNT_ID.to_owned()]);
        assert_eq!(uses, 1);
        assert_ne!(space_after, space_before, "Space change not recorded");
        assert_ne!(
            invite_after, invite_before,
            "SpaceInvite change not recorded"
        );
    }

    /// draft-atwood-jmap-chat-00 §Space/join: "... so that concurrent
    /// redemptions cannot exceed `maxUses`": the backend re-checks
    /// `maxUses` itself rather than trusting the handler's earlier read,
    /// and the refusal writes nothing.
    pub async fn exhausted_invite_is_refused_without_any_write<B: ConformanceBackend>(backend: B) {
        seeded(&backend, 1, 1).await;
        let before = snapshot(&backend).await;

        let refusal = backend
            .join_space_with_invite(
                &owner(&backend),
                &Id::from(ACCOUNT_ID),
                &Id::from(INVITE_ID),
                &UTCDate::from("2026-01-02T00:00:00Z"),
            )
            .await
            .unwrap();

        assert_eq!(refusal, Err(InviteJoinRefusal::Exhausted));
        assert_eq!(snapshot(&backend).await, before);
    }

    /// draft-atwood-jmap-chat-00 §Space/join (jmap-chat-spec 77c3474 L1416):
    /// "The `uses` increment and membership insertion SHOULD be performed
    /// within a single atomic operation", which this crate requires by
    /// choice. Concurrent redemptions are serialized: exactly one
    /// succeeds, `uses` is incremented exactly once and the member appears
    /// exactly once.
    ///
    /// Spawns tokio tasks, so it must run on a multi-thread tokio runtime
    /// such as [`run`](super::run)'s.
    pub async fn concurrent_redemptions_increment_exactly_once<B: ConformanceBackend>(backend: B) {
        const RACERS: usize = 32;
        seeded(&backend, 0, 100).await;
        let backend = Arc::new(backend);

        let tasks: Vec<_> = (0..RACERS)
            .map(|_| {
                let backend = Arc::clone(&backend);
                tokio::spawn(async move {
                    backend
                        .join_space_with_invite(
                            &owner(&*backend),
                            &Id::from(ACCOUNT_ID),
                            &Id::from(INVITE_ID),
                            &UTCDate::from("2026-01-02T00:00:00Z"),
                        )
                        .await
                        .unwrap()
                })
            })
            .collect();
        let mut outcomes = Vec::with_capacity(RACERS);
        for t in tasks {
            outcomes.push(t.await.unwrap());
        }

        assert_eq!(outcomes.iter().filter(|o| o.is_ok()).count(), 1);
        assert!(
            outcomes
                .iter()
                .all(|o| *o == Ok(()) || *o == Err(InviteJoinRefusal::AlreadyMember))
        );
        let (members, uses, _, _) = snapshot(&*backend).await;
        assert_eq!(members, vec![ACCOUNT_ID.to_owned()]);
        assert_eq!(uses, 1);
    }
}

// ---------------------------------------------------------------------------
// Message writes that must be atomic: burn-on-read (bd:JMAP-x2gd.6) and
// `create_message` (bd:JMAP-z4z0.5)
// ---------------------------------------------------------------------------

mod message_atomic {
    use jmap_server::HandlerOutput;
    use jmap_types::{Id, State};
    use serde_json::{Value, json};

    use super::{ACCOUNT_ID, ConformanceBackend, make_received, owner, seed_group_chat, setup};
    use crate::OutboundEvent;
    use crate::message::{handle_message_changes, handle_message_get, handle_message_set};

    /// Store one `burnOnRead` message; returns its id.
    async fn seeded_burn<B: ConformanceBackend>(backend: &B) -> String {
        setup(backend).await;
        seed_group_chat(backend, "c1").await;
        let HandlerOutput { primary: resp, .. } = handle_message_set(
            backend,
            &owner(backend),
            &std::collections::BTreeMap::new(),
            json!({
                "accountId": ACCOUNT_ID,
                "create": {
                    "m0": {
                        "chatId": "c1",
                        "body": "burn me",
                        "sentAt": "2024-01-01T00:00:00Z",
                        "burnOnRead": true
                    }
                }
            }),
            &[],
        )
        .await
        .expect("create");
        let id = resp["created"]["m0"]["id"]
            .as_str()
            .expect("created id")
            .to_owned();
        make_received(backend, &id).await;
        id
    }

    async fn set_update<B: ConformanceBackend>(backend: &B, id: &str, patch: Value) -> Value {
        handle_message_set(
            backend,
            &owner(backend),
            &std::collections::BTreeMap::new(),
            json!({ "accountId": ACCOUNT_ID, "update": { id: patch } }),
            &[],
        )
        .await
        .expect("Message/set")
        .primary
    }

    async fn get<B: ConformanceBackend>(backend: &B, id: &str) -> Value {
        handle_message_get(
            backend,
            &owner(backend),
            json!({ "accountId": ACCOUNT_ID, "ids": [id] }),
            &[],
        )
        .await
        .expect("Message/get")
        .primary
    }

    async fn burn_state<B: ConformanceBackend>(backend: &B, id: &str) -> String {
        get(backend, id).await["state"]
            .as_str()
            .expect("state")
            .to_owned()
    }

    /// Oracle: draft-atwood-jmap-chat-00 (jmap-chat-spec 77c3474 L750)
    /// §Message `burnOnRead`: "When `true`, the receiving server SHOULD
    /// permanently hard-delete this message (row removal, not tombstone)
    /// immediately after setting `readAt`." This crate performs the
    /// recommended deletion, so the `readAt` write is never stored without
    /// it.
    ///
    /// A `readAt` patch the backend cannot store is rejected and burns
    /// nothing. RFC 8620 §5.3: "invalidProperties: The record given is
    /// invalid in some way." `readAt` is a UTCDate, so a number is invalid.
    pub async fn invalid_read_at_patch_is_rejected_without_burning<B: ConformanceBackend>(
        backend: B,
    ) {
        let id = seeded_burn(&backend).await;
        let before = burn_state(&backend, &id).await;

        let resp = set_update(&backend, &id, json!({ "readAt": 42 })).await;
        assert_eq!(
            resp["notUpdated"][&id]["type"], "invalidProperties",
            "resp: {resp}"
        );

        let still = get(&backend, &id).await;
        assert_eq!(still["list"][0]["id"], json!(id), "{still}");
        assert_eq!(still["list"][0]["readAt"], Value::Null, "{still}");
        assert_eq!(still["state"], json!(before), "no state change");
    }

    async fn state<B: ConformanceBackend>(backend: &B) -> State {
        backend
            .get_state::<jmap_chat_types::Message>(&owner(backend), &Id::from(ACCOUNT_ID))
            .await
            .expect("Message state")
    }

    async fn create<B: ConformanceBackend>(backend: &B) -> Value {
        let HandlerOutput { primary, .. } = handle_message_set(
            backend,
            &owner(backend),
            &std::collections::BTreeMap::new(),
            json!({
                "accountId": ACCOUNT_ID,
                "create": { "c": {
                    "chatId": "g1",
                    "body": "hi",
                    "bodyType": "text/plain",
                    "sentAt": "2026-01-02T00:00:00Z",
                } },
            }),
            &[],
        )
        .await
        .expect("Message/set");
        primary
    }

    async fn stored_ids<B: ConformanceBackend>(backend: &B) -> Vec<Value> {
        let HandlerOutput { primary, .. } = handle_message_get(
            backend,
            &owner(backend),
            json!({ "accountId": ACCOUNT_ID, "ids": null, "properties": ["id"] }),
            &[],
        )
        .await
        .expect("Message/get");
        primary["list"]
            .as_array()
            .expect("list")
            .iter()
            .map(|m| m["id"].clone())
            .collect()
    }

    /// Creates one Message and checks the stored Message, the state change
    /// and the single `NewMessage` event against the spec.
    ///
    /// Oracles:
    ///
    /// - draft-atwood-jmap-chat-00 at jmap-chat-spec b1cd625 L1222
    ///   (§Message/set): "The server sets `id`, `senderMsgId`, `senderId`,
    ///   `receivedAt`, `deliveryState`, and delivery timestamp fields, then
    ///   enqueues the message for outbound delivery."
    /// - draft-atwood-jmap-chat-00 §Message `senderMsgId`: "Equals `id` for
    ///   messages composed by the owner."
    ///
    /// Runs only when [`ConformanceBackend::take_outbound_events`] returns
    /// `Some`.
    pub async fn create_message_stores_and_reports_one_new_message<B: ConformanceBackend>(
        backend: B,
    ) {
        setup(&backend).await;
        seed_group_chat(&backend, "g1").await;
        if backend.take_outbound_events().is_none() {
            return;
        }
        let take_events = || {
            backend
                .take_outbound_events()
                .expect("take_outbound_events returned Some before")
        };

        let before = state(&backend).await;
        let resp = create(&backend).await;
        let created = &resp["created"]["c"];
        let id = created["id"].as_str().expect("created id").to_owned();
        assert_eq!(created["senderMsgId"], json!(id), "{resp}");
        assert_eq!(resp["notCreated"], Value::Null, "{resp}");
        assert_eq!(stored_ids(&backend).await, vec![json!(id)]);

        let HandlerOutput {
            primary: changes, ..
        } = handle_message_changes(
            &backend,
            &owner(&backend),
            json!({ "accountId": ACCOUNT_ID, "sinceState": before }),
            &[],
        )
        .await
        .expect("Message/changes");
        assert_eq!(changes["created"], json!([id]), "{changes}");
        assert_eq!(changes["updated"], json!([]), "{changes}");
        assert_eq!(changes["destroyed"], json!([]), "{changes}");

        assert_eq!(
            take_events(),
            vec![OutboundEvent::NewMessage {
                chat_id: Id::from("g1"),
                message_id: Id::from(id.as_str()),
                sender_msg_id: Id::from(id.as_str()),
            }]
        );
    }
}

// ---------------------------------------------------------------------------
// `ChatContact/set` create by `identity` and the `ChatContact/query`
// `identity` filter (bd:JMAP-405z.3).
//
// Oracle: draft-atwood-jmap-chat-00 (jmap-chat-spec cefc217)
// §ChatContact/set and §ChatContact/query, quoted on each test.
// ---------------------------------------------------------------------------

mod contact_identity {
    use jmap_server::HandlerOutput;
    use serde_json::{Value, json};

    use super::{ACCOUNT_ID, ConformanceBackend, owner, seed, setup};
    use crate::{handle_contact_get, handle_contact_query, handle_contact_set};

    /// Store a ChatContact with the given `id` and `identity`.
    async fn insert_contact<B: ConformanceBackend>(backend: &B, id: &str, identity: &str) {
        seed(
            backend,
            "ChatContact",
            json!({
                "id": id,
                "identity": identity,
                "login": identity,
                "firstSeenAt": "2026-01-01T00:00:00Z",
                "lastSeenAt": "2026-01-01T00:00:00Z",
                "blocked": false
            }),
        )
        .await;
    }

    /// `ChatContact/set` with the given `create` map.
    async fn create<B: ConformanceBackend>(backend: &B, create: Value) -> Value {
        let HandlerOutput { primary: resp, .. } = handle_contact_set(
            backend,
            &owner(backend),
            json!({ "accountId": ACCOUNT_ID, "create": create }),
        )
        .await
        .expect("ChatContact/set");
        resp
    }

    /// The sorted `properties` of a SetError.
    fn error_properties(err: &Value) -> Vec<String> {
        let mut props: Vec<String> = err["properties"]
            .as_array()
            .unwrap_or_else(|| panic!("properties missing from {err}"))
            .iter()
            .map(|p| p.as_str().expect("property name").to_owned())
            .collect();
        props.sort();
        props
    }

    /// draft-atwood-jmap-chat-00 §ChatContact/set: "`create` accepts only
    /// `identity`, which is required; the server assigns `id` and sets the
    /// other server-set properties."
    pub async fn contact_create_by_identity_assigns_id<B: ConformanceBackend>(backend: B) {
        setup(&backend).await;
        let resp = create(
            &backend,
            json!({ "k1": { "identity": "alice@example.com" } }),
        )
        .await;

        assert!(
            resp["notCreated"].is_null(),
            "create with identity must succeed: {}",
            resp["notCreated"]
        );
        let id = resp["created"]["k1"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("created.k1.id missing: {resp}"))
            .to_owned();

        let HandlerOutput { primary: get, .. } = handle_contact_get(
            &backend,
            &owner(&backend),
            json!({ "accountId": ACCOUNT_ID, "ids": [&id] }),
        )
        .await
        .expect("ChatContact/get");
        assert_eq!(get["list"][0]["id"], json!(id));
        assert_eq!(get["list"][0]["identity"], json!("alice@example.com"));
    }

    /// draft-atwood-jmap-chat-00 §ChatContact/set: "If a ChatContact with
    /// that `identity` already exists, the server MUST reject the create with
    /// a SetError of type `alreadyExists`, including the existing
    /// ChatContact's id in the `existingId` field ({{RFC8620}} Section 5.3)."
    pub async fn contact_create_existing_identity_already_exists<B: ConformanceBackend>(
        backend: B,
    ) {
        setup(&backend).await;
        insert_contact(&backend, "ct1", "alice@example.com").await;

        let resp = create(
            &backend,
            json!({ "k1": { "identity": "alice@example.com" } }),
        )
        .await;

        assert!(resp["created"]["k1"].is_null(), "duplicate created: {resp}");
        assert_eq!(resp["notCreated"]["k1"]["type"], json!("alreadyExists"));
        assert_eq!(resp["notCreated"]["k1"]["existingId"], json!("ct1"));
    }

    /// draft-atwood-jmap-chat-00 §ChatContact/set: "`create` accepts only
    /// `identity`, which is required". A create without `identity` is an
    /// invalid record (RFC 8620 §5.3 `invalidProperties`).
    pub async fn contact_create_without_identity_invalid_properties<B: ConformanceBackend>(
        backend: B,
    ) {
        setup(&backend).await;
        let resp = create(&backend, json!({ "k1": {} })).await;

        assert!(
            resp["created"]["k1"].is_null(),
            "created without identity: {resp}"
        );
        assert_eq!(resp["notCreated"]["k1"]["type"], json!("invalidProperties"));
        assert_eq!(error_properties(&resp["notCreated"]["k1"]), ["identity"]);
    }

    /// draft-atwood-jmap-chat-00 §ChatContact/set: "`create` accepts only
    /// `identity`". Any other property in a create is rejected with an RFC
    /// 8620 §5.3 `invalidProperties` SetError naming it, and no ChatContact
    /// is recorded.
    pub async fn contact_create_other_properties_invalid_properties<B: ConformanceBackend>(
        backend: B,
    ) {
        setup(&backend).await;
        let resp = create(
            &backend,
            json!({
                "k1": {
                    "identity": "alice@example.com",
                    "login": "alice",
                    "blocked": true,
                    "displayName": "Alice"
                }
            }),
        )
        .await;

        assert!(
            resp["created"]["k1"].is_null(),
            "created with extras: {resp}"
        );
        assert_eq!(resp["notCreated"]["k1"]["type"], json!("invalidProperties"));
        assert_eq!(
            error_properties(&resp["notCreated"]["k1"]),
            ["blocked", "displayName", "login"]
        );

        // The rejected create recorded nothing, so the identity is still free.
        let retry = create(
            &backend,
            json!({ "k2": { "identity": "alice@example.com" } }),
        )
        .await;
        assert!(
            retry["created"]["k2"]["id"].is_string(),
            "identity must still be creatable: {retry}"
        );
    }

    /// draft-atwood-jmap-chat-00 §ChatContact/query: "Filter properties:
    /// `blocked` (Boolean), `presence` (String), `identity` (String; matches
    /// the ChatContact whose `identity` equals the value octet for octet)."
    /// The case-variant contact shows the match is octet-exact.
    pub async fn contact_query_identity_filter_matches_octet_for_octet<B: ConformanceBackend>(
        backend: B,
    ) {
        setup(&backend).await;
        insert_contact(&backend, "ct1", "alice@example.com").await;
        insert_contact(&backend, "ct2", "Alice@example.com").await;
        insert_contact(&backend, "ct3", "bob@example.com").await;

        let HandlerOutput { primary: resp, .. } = handle_contact_query(
            &backend,
            &owner(&backend),
            json!({
                "accountId": ACCOUNT_ID,
                "filter": { "identity": "alice@example.com" }
            }),
        )
        .await
        .expect("ChatContact/query");

        assert_eq!(resp["ids"], json!(["ct1"]));
    }
}

// ---------------------------------------------------------------------------
// Thread counts `replyCount` / `unreadReplyCount` (bd:JMAP-y3f2.3) and
// their `Message/changes` reporting (bd:JMAP-y3f2.15).
//
// Oracles, quoted from draft-atwood-jmap-chat-00.md at jmap-chat-spec
// 91672c9:
//
// - L638-639 `replyCount`: "Count of messages in this chat with
//   `replyTo` equal to this message's `id`. Present only when
//   `supportsThreads` is `true`."
// - L641-642 `unreadReplyCount`: "Count of replies to this message
//   received after the owner's `ReadPosition.lastReadMessageId` in this
//   Chat. Present only when `supportsThreads` is `true` and this message
//   is a thread root (`threadRootId` is absent)."
// - L591: "Message IDs are ULIDs, assigned by the **receiving** mailbox
//   at storage time. ULIDs are lexicographically ordered by time".
// - jmap-chat-spec aa01dd7 L732, `unreadReplyCount`: "Which of those
//   replies count is implementation-defined, as for `Chat.unreadCount`".
//   The fixture Messages come from `OTHER_SENDER`, so every server counts
//   them.
// ---------------------------------------------------------------------------

mod threads {
    use jmap_server::HandlerOutput;
    use serde_json::{Value, json};

    use super::{ACCOUNT_ID, ConformanceBackend, owner, seed, setup, supports_threads};
    use crate::handle_chat_set;
    use crate::handle_position_set;
    use crate::message::{handle_message_changes, handle_message_get, handle_message_set};

    // Hand-picked ULIDs (Crockford base32, 26 chars) in ascending order.
    const ROOT: &str = "01J0000000000000000000000A";
    const A: &str = "01J0000000000000000000000B";
    const B: &str = "01J0000000000000000000000C";
    const C: &str = "01J0000000000000000000000D";
    const OTHER_CHAT_MSG: &str = "01J0000000000000000000000E";

    /// A ChatContact other than the owner; the owner's own Messages carry the
    /// accountId as `senderId`.
    const OTHER_SENDER: &str = "contact-bob";

    async fn create_group<K: ConformanceBackend>(backend: &K) -> String {
        let HandlerOutput { primary: resp, .. } = handle_chat_set(
            backend,
            &owner(backend),
            &std::collections::BTreeMap::new(),
            json!({
                "accountId": ACCOUNT_ID,
                "create": { "g": { "kind": "group", "name": "G", "memberIds": ["contact-bob"] } }
            }),
            &[],
        )
        .await
        .expect("Chat/set");
        resp["created"]["g"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("group not created: {resp}"))
            .to_owned()
    }

    /// Seed one stored Message from `OTHER_SENDER` with a fixed id.
    async fn seed_message<K: ConformanceBackend>(
        backend: &K,
        id: &str,
        chat_id: &str,
        reply_to: Option<&str>,
        root: Option<&str>,
    ) {
        let mut msg = json!({
            "id": id,
            "senderMsgId": id,
            "senderId": OTHER_SENDER,
            "chatId": chat_id,
            "body": "x",
            "bodyType": "text/plain",
            "attachments": [],
            "sentAt": "2024-01-01T00:00:00Z",
            "receivedAt": "2024-01-01T00:00:00Z",
            "deliveryState": "delivered",
        });
        if let Some(r) = reply_to {
            msg["replyTo"] = json!(r);
        }
        if let Some(r) = root {
            msg["threadRootId"] = json!(r);
        }
        seed(backend, "Message", msg).await;
    }

    /// Fixture thread in one Chat:
    ///
    /// ```text
    /// ROOT
    /// ├── A  (replyTo ROOT, threadRootId ROOT)
    /// │   └── B  (replyTo A, threadRootId ROOT)
    /// └── C  (replyTo ROOT, threadRootId ROOT)
    /// ```
    ///
    /// plus OTHER_CHAT_MSG in a second Chat, and a ReadPosition for the
    /// first Chat with `lastReadMessageId` = A.
    async fn fixture<K: ConformanceBackend>(backend: &K) -> (String, String) {
        setup(backend).await;
        let chat = create_group(backend).await;
        let other = create_group(backend).await;
        seed_message(backend, ROOT, &chat, None, None).await;
        seed_message(backend, A, &chat, Some(ROOT), Some(ROOT)).await;
        seed_message(backend, B, &chat, Some(A), Some(ROOT)).await;
        seed_message(backend, C, &chat, Some(ROOT), Some(ROOT)).await;
        seed_message(backend, OTHER_CHAT_MSG, &other, None, None).await;
        seed(
            backend,
            "ReadPosition",
            json!({
                "id": "rp1",
                "chatId": chat,
                "lastReadMessageId": A,
                "lastReadAt": "2024-01-01T00:00:00Z",
            }),
        )
        .await;
        (chat, other)
    }

    async fn create<K: ConformanceBackend>(backend: &K, obj: Value) -> Value {
        let HandlerOutput { primary: resp, .. } = handle_message_set(
            backend,
            &owner(backend),
            &std::collections::BTreeMap::new(),
            json!({ "accountId": ACCOUNT_ID, "create": { "m": obj } }),
            &[],
        )
        .await
        .expect("Message/set");
        resp
    }

    fn reply(chat: &str, reply_to: &str, root: Option<&str>) -> Value {
        let mut obj = json!({
            "chatId": chat,
            "body": "re",
            "bodyType": "text/plain",
            "sentAt": "2024-01-01T00:00:00Z",
            "replyTo": reply_to,
        });
        if let Some(r) = root {
            obj["threadRootId"] = json!(r);
        }
        obj
    }

    async fn get<K: ConformanceBackend>(backend: &K, ids: &[&str]) -> Vec<Value> {
        let HandlerOutput { primary: resp, .. } = handle_message_get(
            backend,
            &owner(backend),
            json!({ "accountId": ACCOUNT_ID, "ids": ids }),
            &[],
        )
        .await
        .expect("Message/get");
        resp["list"].as_array().expect("list").clone()
    }

    fn by_id<'a>(list: &'a [Value], id: &str) -> &'a Value {
        list.iter()
            .find(|m| m["id"] == id)
            .unwrap_or_else(|| panic!("{id} missing from {list:?}"))
    }

    /// L638-642 (jmap-chat-spec 91672c9) against the fixture, counted by
    /// hand:
    ///
    /// - ROOT: replies A and C -> replyCount 2; of those only C sorts after
    ///   lastReadMessageId A (L591) -> unreadReplyCount 1.
    /// - A: reply B -> replyCount 1; A has a threadRootId, so it is not a
    ///   thread root and carries no unreadReplyCount.
    /// - B, C: no replies -> replyCount 0, no unreadReplyCount.
    ///
    /// Runs only when `supportsThreads` is `true`.
    pub async fn reply_counts_match_fixture<K: ConformanceBackend>(backend: K) {
        if !supports_threads(&backend) {
            return;
        }
        let _ = fixture(&backend).await;
        let list = get(&backend, &[ROOT, A, B, C]).await;
        let root = by_id(&list, ROOT);
        assert_eq!(root["replyCount"], 2, "{root}");
        assert_eq!(root["unreadReplyCount"], 1, "{root}");
        let a = by_id(&list, A);
        assert_eq!(a["replyCount"], 1, "{a}");
        assert!(a.get("unreadReplyCount").is_none(), "{a}");
        for id in [B, C] {
            let m = by_id(&list, id);
            assert_eq!(m["replyCount"], 0, "{m}");
            assert!(m.get("unreadReplyCount").is_none(), "{m}");
        }
    }

    /// L641-642 (jmap-chat-spec 91672c9) with no `lastReadMessageId`: no
    /// reply has been read, so the unread count equals the reply count
    /// (ROOT: A and C -> 2).
    ///
    /// Runs only when `supportsThreads` is `true`.
    pub async fn unread_reply_count_without_read_position<K: ConformanceBackend>(backend: K) {
        if !supports_threads(&backend) {
            return;
        }
        let (chat, _) = fixture(&backend).await;
        seed(
            &backend,
            "ReadPosition",
            json!({ "id": "rp1", "chatId": chat }),
        )
        .await;
        let list = get(&backend, &[ROOT]).await;
        assert_eq!(list[0]["unreadReplyCount"], 2, "{list:?}");
    }

    /// L639 / L642 (jmap-chat-spec 91672c9) "Present only when
    /// `supportsThreads` is `true`": with `supportsThreads: false` neither
    /// count is returned.
    ///
    /// Runs only when `supportsThreads` is `false`.
    pub async fn counts_absent_without_supports_threads<K: ConformanceBackend>(backend: K) {
        if supports_threads(&backend) {
            return;
        }
        let _ = fixture(&backend).await;
        let list = get(&backend, &[ROOT, A]).await;
        for m in &list {
            assert!(m.get("replyCount").is_none(), "{m}");
            assert!(m.get("unreadReplyCount").is_none(), "{m}");
        }
    }

    // Count changes advance Message state (bd:JMAP-y3f2.15)
    //
    // RFC 8620 §5.1: "If the data changes, this string MUST change." §5.2
    // `updated`: "An array of Ids for records that have been updated since
    // the old state." `replyCount` and `unreadReplyCount` are Message data
    // (L638-642), so a write that changes them must list the Message.

    async fn message_state<K: ConformanceBackend>(backend: &K) -> String {
        let HandlerOutput { primary: resp, .. } = handle_message_get(
            backend,
            &owner(backend),
            json!({ "accountId": ACCOUNT_ID, "ids": [] }),
            &[],
        )
        .await
        .expect("Message/get");
        resp["state"].as_str().expect("state").to_owned()
    }

    async fn message_changes<K: ConformanceBackend>(backend: &K, since: &str) -> Value {
        let HandlerOutput { primary: resp, .. } = handle_message_changes(
            backend,
            &owner(backend),
            json!({ "accountId": ACCOUNT_ID, "sinceState": since }),
            &[],
        )
        .await
        .expect("Message/changes");
        resp
    }

    /// RFC 8620 §5.1: "If the data changes, this string MUST change."
    /// Creating a reply to A raises A's `replyCount` from 1 to 2 (L638-639),
    /// so `Message/changes` lists A as updated and the reply as created.
    ///
    /// Runs only when `supportsThreads` is `true`.
    pub async fn reply_create_lists_parent_updated<K: ConformanceBackend>(backend: K) {
        if !supports_threads(&backend) {
            return;
        }
        let (chat, _) = fixture(&backend).await;
        let since = message_state(&backend).await;
        let resp = create(&backend, reply(&chat, A, None)).await;
        let new_id = resp["created"]["m"]["id"].clone();
        let changes = message_changes(&backend, &since).await;
        assert_eq!(changes["created"], json!([new_id]), "{changes}");
        assert_eq!(changes["updated"], json!([A]), "{changes}");
    }

    /// RFC 8620 §5.1: "If the data changes, this string MUST change."
    /// Destroying B, the only reply to A, lowers A's `replyCount` from 1 to
    /// 0, so `Message/changes` lists A as updated and B as destroyed.
    ///
    /// Runs only when `supportsThreads` is `true`.
    pub async fn reply_destroy_lists_parent_updated<K: ConformanceBackend>(backend: K) {
        if !supports_threads(&backend) {
            return;
        }
        let _ = fixture(&backend).await;
        let since = message_state(&backend).await;
        let HandlerOutput { primary: resp, .. } = handle_message_set(
            &backend,
            &owner(&backend),
            &std::collections::BTreeMap::new(),
            json!({ "accountId": ACCOUNT_ID, "destroy": [B] }),
            &[],
        )
        .await
        .expect("Message/set");
        assert_eq!(resp["destroyed"], json!([B]), "{resp}");
        let changes = message_changes(&backend, &since).await;
        assert_eq!(changes["destroyed"], json!([B]), "{changes}");
        assert_eq!(changes["updated"], json!([A]), "{changes}");
    }

    /// RFC 8620 §5.1: "If the data changes, this string MUST change."
    /// Moving `lastReadMessageId` from A to C (L641-642, ids ordered per
    /// L591) marks reply C to ROOT read, so ROOT's `unreadReplyCount` drops
    /// from 1 to 0 and ROOT is listed as updated. B also crosses the read
    /// mark, but its parent A has a `threadRootId`, carries no
    /// `unreadReplyCount`, and is not listed.
    ///
    /// Runs only when `supportsThreads` is `true`.
    pub async fn read_position_update_lists_thread_root_updated<K: ConformanceBackend>(backend: K) {
        if !supports_threads(&backend) {
            return;
        }
        let _ = fixture(&backend).await;
        let since = message_state(&backend).await;
        let HandlerOutput { primary: resp, .. } = handle_position_set(
            &backend,
            &owner(&backend),
            &std::collections::BTreeMap::new(),
            json!({
                "accountId": ACCOUNT_ID,
                "update": { "rp1": { "lastReadMessageId": C } },
            }),
        )
        .await
        .expect("ReadPosition/set");
        assert!(resp["updated"].get("rp1").is_some(), "{resp}");
        let changes = message_changes(&backend, &since).await;
        assert_eq!(changes["updated"], json!([ROOT]), "{changes}");
        assert_eq!(changes["created"], json!([]), "{changes}");
        assert_eq!(changes["destroyed"], json!([]), "{changes}");
    }
}

// ---------------------------------------------------------------------------
// `sweep_expired` applies the draft's time-driven expiry rules against an
// injected clock (bd:JMAP-y3f2.6).
//
// Oracles, from jmap-chat-spec 91672c9, draft-atwood-jmap-chat-00.md,
// except the three hard-delete rules, which jmap-chat-spec 4ffb96e
// downgraded from MUST to SHOULD and are quoted from 77c3474. The
// deletions stay required here: this crate's `sweep_expired` performs
// them, its choice of the recommended behaviour.
//
// - L559, §Chat `muteUntil`: "Muting expires at this time. Servers SHOULD
//   clear `muted` and `muteUntil` automatically when the time passes."
// - L581, §Chat `messageExpirySeconds`: "When set and non-zero, messages
//   in this chat older than this many seconds are deleted by this
//   mailbox."
// - L654, §Message `receivedAt`: "Authoritative for ordering and expiry
//   calculations."
// - 77c3474 L747, §Message `senderExpiresAt`: "servers SHOULD
//   permanently delete this message — removing the row entirely, not
//   leaving a tombstone — at or before this time. A hard-deleted message
//   appears in the `destroyed` list of subsequent `Message/changes`
//   responses, not `updated`."
// - 77c3474 L750, §Message `burnOnRead`: "the receiving server SHOULD
//   permanently hard-delete this message (row removal, not tombstone)
//   immediately after setting `readAt`."
// - 77c3474 L753, §Message `expiresAfterReadSeconds`: "the receiving
//   server SHOULD permanently hard-delete this message (row removal, not
//   tombstone) this many seconds after setting `readAt`."
// - 77c3474 L750 and L753, both rules: "Applies only to the receiving
//   mailbox; the sender's own copy is not affected."
// - L941-942, §PresenceStatus `expiresAt`: "If set, the server SHOULD
//   clear `statusText` and `statusEmoji` (setting them to null) at this
//   time, and SHOULD reset `presence` to `"online"`."
//
// - jmap-chat-spec aa01dd7 L635, §Chat `unreadCount`: "Candidates are
//   the Messages whose `id` ULID is lexicographically greater than
//   `ReadPosition.lastReadMessageId`, or all Messages in the Chat if
//   `lastReadMessageId` is absent." An expired Message
//   lowers that count, and RFC 8620 §5.1 ("If the data changes, this
//   string MUST change") then requires the `Chat` state to move
//   (bd:JMAP-y3f2.18).
//
// Clock values are hand-computed: 2024-01-01T00:00:00Z is Unix time
// 1_704_067_200.
// ---------------------------------------------------------------------------

mod expiry {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use jmap_types::Id;
    use serde_json::{Value, json};

    use super::{ACCOUNT_ID, ConformanceBackend, exact_unread_count, owner, seed, setup};
    use crate::message::{handle_message_changes, handle_message_get};
    use crate::{handle_chat_changes, handle_chat_get, handle_presence_get, sweep_expired};

    /// 2024-01-01T00:00:00Z.
    const T0: u64 = 1_704_067_200;
    const T0_STR: &str = "2024-01-01T00:00:00Z";

    fn at(offset_secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(T0 + offset_secs)
    }

    async fn seed_chat<B: ConformanceBackend>(b: &B, id: &str, extra: Value) {
        let mut chat = json!({
            "id": id,
            "kind": "direct",
            "contactId": "contact1",
            "createdAt": T0_STR,
            "unreadCount": 0,
            "pinnedMessageIds": [],
        });
        chat.as_object_mut()
            .expect("object literal")
            .extend(extra.as_object().expect("object literal").clone());
        seed(b, "Chat", chat).await;
    }

    async fn seed_message<B: ConformanceBackend>(b: &B, id: &str, chat_id: &str, extra: Value) {
        let mut msg = json!({
            "id": id,
            "senderMsgId": id,
            "senderId": "other",
            "chatId": chat_id,
            "body": "x",
            "bodyType": "text/plain",
            "attachments": [],
            "sentAt": T0_STR,
            "receivedAt": T0_STR,
            "deliveryState": "delivered",
        });
        msg.as_object_mut()
            .expect("object literal")
            .extend(extra.as_object().expect("object literal").clone());
        seed(b, "Message", msg).await;
    }

    async fn sweep<B: ConformanceBackend>(b: &B, now: SystemTime) {
        sweep_expired(b, &owner(b), &Id::from(ACCOUNT_ID), now)
            .await
            .expect("sweep");
    }

    async fn message_exists<B: ConformanceBackend>(b: &B, id: &str) -> bool {
        let resp = handle_message_get(
            b,
            &owner(b),
            json!({ "accountId": ACCOUNT_ID, "ids": [id] }),
            &[],
        )
        .await
        .expect("Message/get")
        .primary;
        resp["list"].as_array().expect("list").len() == 1
    }

    async fn chat<B: ConformanceBackend>(b: &B, id: &str) -> Value {
        let resp = handle_chat_get(
            b,
            &owner(b),
            json!({ "accountId": ACCOUNT_ID, "ids": [id] }),
            &[],
        )
        .await
        .expect("Chat/get")
        .primary;
        resp["list"][0].clone()
    }

    /// L747 (jmap-chat-spec 77c3474), §Message `senderExpiresAt`: "servers
    /// SHOULD permanently delete this message — removing the row entirely,
    /// not leaving a tombstone — at or before this time. A hard-deleted
    /// message appears in the `destroyed` list of subsequent
    /// `Message/changes` responses, not `updated`."
    ///
    /// L747: deletion "at or before this time", and the id lands in
    /// `destroyed`, not `updated`.
    pub async fn sender_expires_at_hard_deletes_at_deadline<B: ConformanceBackend>(b: B) {
        setup(&b).await;
        seed_chat(&b, "c1", json!({})).await;
        seed_message(
            &b,
            "m1",
            "c1",
            json!({ "senderExpiresAt": "2024-01-01T00:00:10Z" }),
        )
        .await;
        let state = handle_message_get(
            &b,
            &owner(&b),
            json!({ "accountId": ACCOUNT_ID, "ids": [] }),
            &[],
        )
        .await
        .expect("Message/get")
        .primary["state"]
            .clone();

        sweep(&b, at(9)).await;
        assert!(
            message_exists(&b, "m1").await,
            "deadline not reached at T+9"
        );

        sweep(&b, at(10)).await;
        assert!(!message_exists(&b, "m1").await, "deleted at the deadline");
        let changes = handle_message_changes(
            &b,
            &owner(&b),
            json!({ "accountId": ACCOUNT_ID, "sinceState": state }),
            &[],
        )
        .await
        .expect("Message/changes")
        .primary;
        assert_eq!(changes["destroyed"], json!(["m1"]));
        assert_eq!(changes["updated"], json!([]));
    }

    /// L581 (jmap-chat-spec 91672c9), §Chat `messageExpirySeconds`: "When
    /// set and non-zero, messages in this chat older than this many seconds
    /// are deleted by this mailbox." L654, §Message `receivedAt`:
    /// "Authoritative for ordering and expiry calculations."
    ///
    /// L581 + L654: "older than this many seconds", measured from
    /// `receivedAt`; a zero policy deletes nothing ("When set and non-zero").
    pub async fn message_expiry_seconds_deletes_messages_older_than_policy<
        B: ConformanceBackend,
    >(
        b: B,
    ) {
        setup(&b).await;
        seed_chat(&b, "c1", json!({ "messageExpirySeconds": 60 })).await;
        seed_chat(&b, "c0", json!({ "messageExpirySeconds": 0 })).await;
        seed_message(&b, "m1", "c1", json!({})).await;
        seed_message(&b, "m0", "c0", json!({})).await;

        sweep(&b, at(60)).await;
        assert!(
            message_exists(&b, "m1").await,
            "exactly 60s old is not older than 60s"
        );

        sweep(&b, at(61)).await;
        assert!(!message_exists(&b, "m1").await, "61s old is older than 60s");
        assert!(
            message_exists(&b, "m0").await,
            "zero policy deletes nothing"
        );
    }

    /// L753 (jmap-chat-spec 77c3474), §Message `expiresAfterReadSeconds`:
    /// "the receiving server SHOULD permanently hard-delete this message (row
    /// removal, not tombstone) this many seconds after setting `readAt`."
    ///
    /// L753: deletion "this many seconds after setting `readAt`"; an unread
    /// message has no such deadline.
    pub async fn expires_after_read_seconds_counts_from_read_at<B: ConformanceBackend>(b: B) {
        setup(&b).await;
        seed_chat(&b, "c1", json!({})).await;
        seed_message(
            &b,
            "read",
            "c1",
            json!({ "expiresAfterReadSeconds": 30, "readAt": "2024-01-01T00:01:40Z" }),
        )
        .await;
        seed_message(&b, "unread", "c1", json!({ "expiresAfterReadSeconds": 30 })).await;

        sweep(&b, at(129)).await;
        assert!(message_exists(&b, "read").await, "29s after readAt");

        sweep(&b, at(130)).await;
        assert!(!message_exists(&b, "read").await, "30s after readAt");
        assert!(
            message_exists(&b, "unread").await,
            "never read, never expires"
        );
    }

    /// L750 (jmap-chat-spec 77c3474), §Message `burnOnRead`: "the receiving
    /// server SHOULD permanently hard-delete this message (row removal, not
    /// tombstone) immediately after setting `readAt`."
    ///
    /// L750: a read `burnOnRead` message must not survive (this crate
    /// requires the recommended deletion); the sweep deletes
    /// one stored with `readAt` set.
    pub async fn read_burn_on_read_message_is_deleted<B: ConformanceBackend>(b: B) {
        setup(&b).await;
        seed_chat(&b, "c1", json!({})).await;
        seed_message(
            &b,
            "burn",
            "c1",
            json!({ "burnOnRead": true, "readAt": T0_STR }),
        )
        .await;
        seed_message(&b, "unread", "c1", json!({ "burnOnRead": true })).await;

        sweep(&b, at(0)).await;
        assert!(!message_exists(&b, "burn").await);
        assert!(message_exists(&b, "unread").await, "not yet read");
    }

    /// L660 and L663 (jmap-chat-spec 91672c9), both rules: "Applies only to
    /// the receiving mailbox; the sender's own copy is not affected."
    ///
    /// L660 + L663: "the sender's own copy is not affected", so an own-sent
    /// message (`senderId` is the account) survives with `readAt` set.
    pub async fn read_rules_spare_the_senders_own_copy<B: ConformanceBackend>(b: B) {
        setup(&b).await;
        seed_chat(&b, "c1", json!({})).await;
        seed_message(
            &b,
            "own-timed",
            "c1",
            json!({
                "senderId": ACCOUNT_ID,
                "expiresAfterReadSeconds": 30,
                "readAt": T0_STR,
            }),
        )
        .await;
        seed_message(
            &b,
            "own-burn",
            "c1",
            json!({ "senderId": ACCOUNT_ID, "burnOnRead": true, "readAt": T0_STR }),
        )
        .await;

        sweep(&b, at(3600)).await;
        assert!(message_exists(&b, "own-timed").await, "own copy kept");
        assert!(message_exists(&b, "own-burn").await, "own copy kept");
    }

    /// L559 (jmap-chat-spec 91672c9), §Chat `muteUntil`: "Muting expires at
    /// this time. Servers SHOULD clear `muted` and `muteUntil`
    /// automatically when the time passes."
    ///
    /// L559: `muted` and `muteUntil` are cleared once the time passes.
    pub async fn mute_until_clears_muted_and_mute_until<B: ConformanceBackend>(b: B) {
        setup(&b).await;
        seed_chat(
            &b,
            "c1",
            json!({ "muted": true, "muteUntil": "2024-01-01T00:00:05Z" }),
        )
        .await;

        sweep(&b, at(4)).await;
        let before = chat(&b, "c1").await;
        assert_eq!(before["muted"], json!(true));
        assert_eq!(before["muteUntil"], json!("2024-01-01T00:00:05Z"));

        sweep(&b, at(5)).await;
        let after = chat(&b, "c1").await;
        assert_eq!(after["muted"], json!(false));
        assert!(
            after.get("muteUntil").is_none_or(Value::is_null),
            "muteUntil cleared: {after}"
        );
    }

    /// L941-942 (jmap-chat-spec 91672c9), §PresenceStatus `expiresAt`: "If
    /// set, the server SHOULD clear `statusText` and `statusEmoji` (setting
    /// them to null) at this time, and SHOULD reset `presence` to
    /// `"online"`."
    ///
    /// L941-942: at `expiresAt` the status text and emoji are nulled and
    /// presence resets to "online".
    pub async fn presence_expires_at_resets_status<B: ConformanceBackend>(b: B) {
        setup(&b).await;
        seed(
            &b,
            "PresenceStatus",
            json!({
                "id": "p1",
                "presence": "busy",
                "receiptSharing": true,
                "updatedAt": T0_STR,
                "statusText": "In a meeting",
                "statusEmoji": ":calendar:",
                "expiresAt": "2024-01-01T01:00:00Z",
            }),
        )
        .await;
        let get = || async {
            handle_presence_get(
                &b,
                &owner(&b),
                json!({ "accountId": ACCOUNT_ID, "ids": ["p1"] }),
            )
            .await
            .expect("PresenceStatus/get")
            .primary["list"][0]
                .clone()
        };

        sweep(&b, at(3599)).await;
        let before = get().await;
        assert_eq!(before["presence"], json!("busy"));
        assert_eq!(before["statusText"], json!("In a meeting"));

        sweep(&b, at(3600)).await;
        let after = get().await;
        assert_eq!(after["presence"], json!("online"));
        assert!(
            after.get("statusText").is_none_or(Value::is_null),
            "{after}"
        );
        assert!(
            after.get("statusEmoji").is_none_or(Value::is_null),
            "{after}"
        );
    }

    async fn chat_state<B: ConformanceBackend>(b: &B) -> Value {
        handle_chat_get(
            b,
            &owner(b),
            json!({ "accountId": ACCOUNT_ID, "ids": [] }),
            &[],
        )
        .await
        .expect("Chat/get")
        .primary["state"]
            .clone()
    }

    /// jmap-chat-spec aa01dd7 L635, §Chat `unreadCount`: "Candidates are
    /// the Messages whose `id` ULID is lexicographically greater than
    /// `ReadPosition.lastReadMessageId`, or all Messages in the Chat if
    /// `lastReadMessageId` is absent." An expired Message lowers that
    /// count, and RFC 8620 §5.1 ("If the data changes, this string MUST
    /// change") then requires the `Chat` state to move (bd:JMAP-y3f2.18).
    ///
    /// L547-548 with RFC 8620 §5.1/§5.2: expiring an unread Message lowers
    /// its Chat's `unreadCount`, so `Chat/changes` lists exactly that Chat;
    /// a re-run at the same time changes nothing.
    ///
    /// Runs only when `maintains_unread_count` is `false`.
    pub async fn expiry_records_chat_unread_count_change<B: ConformanceBackend>(b: B) {
        if !exact_unread_count(&b) {
            return;
        }
        setup(&b).await;
        seed_chat(&b, "c1", json!({})).await;
        seed_chat(&b, "c2", json!({})).await;
        seed_message(
            &b,
            "m1",
            "c1",
            json!({ "senderExpiresAt": "2024-01-01T00:00:10Z" }),
        )
        .await;
        seed_message(&b, "m2", "c2", json!({})).await;
        let since = chat_state(&b).await;

        sweep(&b, at(10)).await;

        assert_eq!(chat(&b, "c1").await["unreadCount"], json!(0));
        assert_eq!(chat(&b, "c2").await["unreadCount"], json!(1));
        let changes = handle_chat_changes(
            &b,
            &owner(&b),
            json!({ "accountId": ACCOUNT_ID, "sinceState": since }),
            &[],
        )
        .await
        .expect("Chat/changes")
        .primary;
        assert_eq!(changes["updated"], json!(["c1"]), "{changes}");

        let after = chat_state(&b).await;
        sweep(&b, at(10)).await;
        assert_eq!(chat_state(&b).await, after, "re-run changes no state");
    }
}

// ---------------------------------------------------------------------------
// `Chat.unreadCount` and the ReadPosition lifecycle (bd:JMAP-y3f2.4,
// bd:JMAP-0x0q.56).
//
// Oracles, from jmap-chat-spec 33702a5:
//
// - draft-atwood-jmap-chat-00.md L629 (aa01dd7 L635), §Chat
//   `unreadCount`: "The number of unread Messages in this Chat, derived
//   server-side from the owner's ReadPosition record. Candidates are the
//   Messages whose `id` ULID is lexicographically greater than
//   `ReadPosition.lastReadMessageId`, or all Messages in the Chat if
//   `lastReadMessageId` is absent. [...] The value is advisory [...] Servers
//   SHOULD keep it accurate, but MAY update it some time after the change
//   that affects it [...] Whenever the server changes the value, the
//   Chat's state changes as for any other property ({{RFC8620}} Section
//   5.1)." The exact-count tests run only where this crate keeps the
//   count exact (`maintains_unread_count` is `false`).
// - jmap-chat-compliance-tests.md §11.1: "Send 5 messages. Set
//   ReadPosition to message 3. Call `Chat/get`. Verify `unreadCount` is
//   2 (messages 4 and 5)."
// - L1434, §ReadPosition/set: "`update` supports: `lastReadMessageId`.
//   The server sets `lastReadAt` to the current time and updates
//   `Chat.unreadCount` for the affected Chat, possibly later ({{chat}})."
// - L993, §ReadPosition: "The server creates a ReadPosition record
//   automatically when a Chat first becomes visible to the owner, and
//   destroys it when the Chat is destroyed. [...] For channel Chats, a
//   ReadPosition is created when the owner joins the containing Space."
// - L1004, `lastReadAt` "(UTCDate, server-set)"; RFC 8620 §5.3 rejects a
//   client write of a server-set property with `invalidProperties`.
// - RFC 8620 §5.1: "If the data changes, this string MUST change", so
//   `ReadPosition/changes` reports the cascaded destroy, and a change
//   to the derived `unreadCount` advances the `Chat` state.
// - RFC 8620 §5.2: `updated` lists "the ids for records that have been
//   updated since the old state", so `Chat/changes` names the Chat
//   whose `unreadCount` moved.
// - jmap-chat-spec aa01dd7 L635, §Chat `unreadCount`: "Which candidates
//   count is implementation-defined; for example, a server MAY exclude
//   Messages the owner sent." The seeded Messages come from
//   `OTHER_SENDER`, so the L629 counts apply to every server.
// ---------------------------------------------------------------------------

mod read_position {
    use jmap_chat_types::{DeliveryState, Message};
    use jmap_server::HandlerOutput;
    use jmap_types::{Id, UTCDate};
    use serde_json::{Value, json};

    use super::{ACCOUNT_ID, ConformanceBackend, exact_unread_count, owner, seed, setup};
    use crate::message::{StoreOutcome, handle_message_set, store_server_message};
    use crate::{
        handle_chat_changes, handle_chat_get, handle_chat_set, handle_position_changes,
        handle_position_get, handle_position_set, handle_space_join, handle_space_set,
    };

    // Hand-picked ULIDs (Crockford base32, 26 chars) in ascending order:
    // "ULIDs are lexicographically ordered by time" (draft L675).
    const M1: &str = "01J0000000000000000000000A";
    const M2: &str = "01J0000000000000000000000B";
    const M3: &str = "01J0000000000000000000000C";
    const M4: &str = "01J0000000000000000000000D";
    const M5: &str = "01J0000000000000000000000E";
    const OTHER_MSG: &str = "01J0000000000000000000000F";

    /// A ChatContact other than the owner; the owner's own Messages carry the
    /// accountId as `senderId`.
    const OTHER_SENDER: &str = "contact-bob";

    const SEEDED_READ_AT: &str = "2000-01-01T00:00:00Z";
    const SPACE_ID: &str = "s1";
    const OTHER_SPACE_ID: &str = "space2";

    async fn seed_chat<B: ConformanceBackend>(backend: &B, id: &str, extra: Value) {
        let mut chat = json!({
            "id": id,
            "createdAt": "2026-01-01T00:00:00Z",
            "unreadCount": 0,
            "pinnedMessageIds": [],
        });
        let (Value::Object(chat_map), Value::Object(extra_map)) = (&mut chat, extra) else {
            unreachable!("both are object literals");
        };
        chat_map.extend(extra_map);
        seed(backend, "Chat", chat).await;
    }

    async fn seed_group<B: ConformanceBackend>(backend: &B, id: &str) {
        seed_chat(
            backend,
            id,
            json!({
                "kind": "group",
                "name": "G",
                "members": [{ "id": ACCOUNT_ID, "role": "admin", "joinedAt": "2026-01-01T00:00:00Z" }],
            }),
        )
        .await;
    }

    async fn seed_message<B: ConformanceBackend>(backend: &B, id: &str, chat_id: &str) {
        seed(
            backend,
            "Message",
            json!({
                "id": id,
                "senderMsgId": id,
                "senderId": OTHER_SENDER,
                "chatId": chat_id,
                "body": "x",
                "bodyType": "text/plain",
                "attachments": [],
                "sentAt": "2026-01-01T00:00:00Z",
                "receivedAt": "2026-01-01T00:00:00Z",
                "deliveryState": "delivered",
            }),
        )
        .await;
    }

    async fn seed_position<B: ConformanceBackend>(backend: &B, id: &str, chat_id: &str) {
        seed(
            backend,
            "ReadPosition",
            json!({ "id": id, "chatId": chat_id, "lastReadAt": SEEDED_READ_AT }),
        )
        .await;
    }

    /// Group Chat `g` with M1..M5 and its ReadPosition `rp-g` (no
    /// `lastReadMessageId`), plus group Chat `h` with OTHER_MSG.
    async fn seed_five<B: ConformanceBackend>(backend: &B) {
        setup(backend).await;
        seed_group(backend, "g").await;
        seed_group(backend, "h").await;
        for m in [M1, M2, M3, M4, M5] {
            seed_message(backend, m, "g").await;
        }
        seed_message(backend, OTHER_MSG, "h").await;
        seed_position(backend, "rp-g", "g").await;
    }

    async fn unread_counts<B: ConformanceBackend>(backend: &B, args: Value) -> Vec<Value> {
        let HandlerOutput { primary: resp, .. } =
            handle_chat_get(backend, &owner(backend), args, &[])
                .await
                .expect("Chat/get");
        resp["list"].as_array().expect("list").clone()
    }

    async fn unread_of<B: ConformanceBackend>(backend: &B, chat_id: &str) -> Value {
        let list = unread_counts(
            backend,
            json!({ "accountId": ACCOUNT_ID, "ids": [chat_id] }),
        )
        .await;
        assert_eq!(list.len(), 1, "{list:?}");
        list[0]["unreadCount"].clone()
    }

    async fn set_last_read<B: ConformanceBackend>(backend: &B, rp: &str, patch: Value) -> Value {
        let HandlerOutput { primary: resp, .. } = handle_position_set(
            backend,
            &owner(backend),
            &std::collections::BTreeMap::new(),
            json!({ "accountId": ACCOUNT_ID, "update": { rp: patch } }),
        )
        .await
        .expect("ReadPosition/set");
        resp
    }

    async fn positions<B: ConformanceBackend>(backend: &B) -> Vec<Value> {
        let HandlerOutput { primary: resp, .. } =
            handle_position_get(backend, &owner(backend), json!({ "accountId": ACCOUNT_ID }))
                .await
                .expect("ReadPosition/get");
        resp["list"].as_array().expect("list").clone()
    }

    async fn position_chat_ids<B: ConformanceBackend>(backend: &B) -> Vec<String> {
        let mut ids: Vec<String> = positions(backend)
            .await
            .iter()
            .map(|p| p["chatId"].as_str().expect("chatId").to_owned())
            .collect();
        ids.sort();
        ids
    }

    async fn position_state<B: ConformanceBackend>(backend: &B) -> String {
        let HandlerOutput { primary: resp, .. } = handle_position_get(
            backend,
            &owner(backend),
            json!({ "accountId": ACCOUNT_ID, "ids": [] }),
        )
        .await
        .expect("ReadPosition/get");
        resp["state"].as_str().expect("state").to_owned()
    }

    async fn destroyed_positions_since<B: ConformanceBackend>(
        backend: &B,
        since: &str,
    ) -> Vec<Value> {
        let HandlerOutput { primary: resp, .. } = handle_position_changes(
            backend,
            &owner(backend),
            json!({ "accountId": ACCOUNT_ID, "sinceState": since }),
        )
        .await
        .expect("ReadPosition/changes");
        resp["destroyed"].as_array().expect("destroyed").clone()
    }

    /// aa01dd7 L635: candidates are "all Messages in the Chat if
    /// `lastReadMessageId` is absent." Only Messages "in this Chat" count, so
    /// `h` has 1.
    ///
    /// Runs only when `maintains_unread_count` is `false`.
    pub async fn absent_last_read_counts_every_message<B: ConformanceBackend>(backend: B) {
        if !exact_unread_count(&backend) {
            return;
        }
        seed_five(&backend).await;
        assert_eq!(unread_of(&backend, "g").await, 5);
        assert_eq!(unread_of(&backend, "h").await, 1);
    }

    /// Compliance §11.1: "Send 5 messages. Set ReadPosition to message 3.
    /// Call `Chat/get`. Verify `unreadCount` is 2 (messages 4 and 5)."
    ///
    /// Runs only when `maintains_unread_count` is `false`.
    pub async fn compliance_11_1_unread_count_from_read_position<B: ConformanceBackend>(
        backend: B,
    ) {
        if !exact_unread_count(&backend) {
            return;
        }
        seed_five(&backend).await;

        let resp = set_last_read(&backend, "rp-g", json!({ "lastReadMessageId": M3 })).await;
        assert!(resp["updated"].get("rp-g").is_some(), "{resp}");

        assert_eq!(unread_of(&backend, "g").await, 2);
        assert_eq!(unread_of(&backend, "h").await, 1, "other Chat unaffected");

        // L1434: the count is recomputed on each ReadPosition/set.
        set_last_read(&backend, "rp-g", json!({ "lastReadMessageId": M5 })).await;
        assert_eq!(unread_of(&backend, "g").await, 0);
    }

    /// Compliance §11.1 with server-assigned ids: "Send 5 messages. Set
    /// ReadPosition to message 3. Call `Chat/get`. Verify `unreadCount` is
    /// 2 (messages 4 and 5)." The count holds only if, per L675, "Message
    /// IDs are ULIDs \[...\] lexicographically ordered by time", including
    /// for messages sent within the same millisecond (bd:JMAP-nls1).
    ///
    /// Runs only when `maintains_unread_count` is `false`.
    pub async fn compliance_11_1_with_assigned_ids<B: ConformanceBackend>(backend: B) {
        // ULID spec: 26 chars of Crockford base32 (no I, L, O, U).
        const CROCKFORD: &str = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
        if !exact_unread_count(&backend) {
            return;
        }
        setup(&backend).await;
        seed_group(&backend, "s").await;
        seed_position(&backend, "rp-s", "s").await;

        // Received from OTHER_SENDER: the owner's own Messages are not counted
        // (implementation-defined, chat-00 aa01dd7 L635), and
        // `store_server_message` has the backend assign each id as
        // `Message/set` does.
        let mut sent = Vec::new();
        for n in 0..5 {
            let smid = Id::from(format!("smid-{n}"));
            let outcome = store_server_message(
                &backend,
                &owner(&backend),
                &Id::from(ACCOUNT_ID),
                &Id::from("s"),
                &smid,
                |id: &Id| {
                    Message::new(
                        id.clone(),
                        Id::from("unused"),
                        Id::from(OTHER_SENDER),
                        Id::from("unused"),
                        "hi",
                        "text/plain",
                        UTCDate::from("2026-01-01T00:00:00Z"),
                        UTCDate::from("2026-01-01T00:00:00Z"),
                        DeliveryState::Delivered,
                    )
                },
            )
            .await
            .expect("store_server_message");
            let StoreOutcome::Stored(id) = outcome else {
                panic!("message {n} reported as a duplicate");
            };
            let id = id.as_ref();
            assert_eq!(id.len(), 26, "{id}");
            assert!(id.chars().all(|c| CROCKFORD.contains(c)), "{id}");
            sent.push(id.to_owned());
        }

        set_last_read(&backend, "rp-s", json!({ "lastReadMessageId": sent[2] })).await;
        assert_eq!(unread_of(&backend, "s").await, 2, "{sent:?}");
    }

    /// aa01dd7 L635: `unreadCount` is computed for a full `Chat/get` (no
    /// `ids`) as well.
    ///
    /// Runs only when `maintains_unread_count` is `false`.
    pub async fn unread_count_in_full_get<B: ConformanceBackend>(backend: B) {
        if !exact_unread_count(&backend) {
            return;
        }
        seed_five(&backend).await;
        set_last_read(&backend, "rp-g", json!({ "lastReadMessageId": M4 })).await;

        let list = unread_counts(&backend, json!({ "accountId": ACCOUNT_ID })).await;
        let count = |id: &str| {
            list.iter()
                .find(|c| c["id"] == id)
                .unwrap_or_else(|| panic!("{id} missing: {list:?}"))["unreadCount"]
                .clone()
        };
        assert_eq!(count("g"), 1);
        assert_eq!(count("h"), 1);
    }

    /// L1434: "The server sets `lastReadAt` to the current time".
    pub async fn read_position_set_updates_last_read_at<B: ConformanceBackend>(backend: B) {
        seed_five(&backend).await;

        set_last_read(&backend, "rp-g", json!({ "lastReadMessageId": M2 })).await;
        let list = positions(&backend).await;
        let rp = list.iter().find(|p| p["id"] == "rp-g").expect("rp-g");
        assert_eq!(rp["lastReadMessageId"], M2, "{rp}");
        let read_at = rp["lastReadAt"].as_str().expect("lastReadAt is a UTCDate");
        assert_ne!(read_at, SEEDED_READ_AT, "{rp}");
        assert!(
            read_at > "2026-01-01T00:00:00Z",
            "{read_at} is not the current time"
        );
    }

    /// L1004 `lastReadAt` is server-set: a client write is `invalidProperties`
    /// (RFC 8620 §5.3) and the record is unchanged.
    pub async fn client_last_read_at_is_rejected<B: ConformanceBackend>(backend: B) {
        seed_five(&backend).await;

        let resp = set_last_read(
            &backend,
            "rp-g",
            json!({ "lastReadMessageId": M2, "lastReadAt": "2030-01-01T00:00:00Z" }),
        )
        .await;
        assert_eq!(
            resp["notUpdated"]["rp-g"]["type"], "invalidProperties",
            "{resp}"
        );
        assert_eq!(
            resp["notUpdated"]["rp-g"]["properties"],
            json!(["lastReadAt"]),
            "{resp}"
        );
        let list = positions(&backend).await;
        assert_eq!(list[0]["lastReadAt"], SEEDED_READ_AT, "{list:?}");
        assert!(list[0].get("lastReadMessageId").is_none(), "{list:?}");
    }

    /// L993: the server "destroys it when the Chat is destroyed"; RFC 8620
    /// §5.1 makes `ReadPosition/changes` report it.
    pub async fn chat_destroy_destroys_its_read_position<B: ConformanceBackend>(backend: B) {
        seed_five(&backend).await;
        seed_position(&backend, "rp-h", "h").await;
        let since = position_state(&backend).await;

        let HandlerOutput { primary: resp, .. } = handle_chat_set(
            &backend,
            &owner(&backend),
            &std::collections::BTreeMap::new(),
            json!({ "accountId": ACCOUNT_ID, "destroy": ["g"] }),
            &[],
        )
        .await
        .expect("Chat/set");
        assert_eq!(resp["destroyed"], json!(["g"]), "{resp}");

        assert_eq!(position_chat_ids(&backend).await, vec!["h"]);
        assert_eq!(
            destroyed_positions_since(&backend, &since).await,
            vec![json!("rp-g")]
        );
    }

    /// A Space whose only member is `preseeded-admin`, holding channels
    /// `ch1` and `ch2`, plus channel `other` of `OTHER_SPACE_ID`.
    async fn seed_space_with_channels<B: ConformanceBackend>(
        backend: &B,
        is_public: bool,
        owner_member: bool,
    ) {
        setup(backend).await;
        let mut members = vec![json!({
            "id": "preseeded-admin", "roleIds": ["r-admin"], "joinedAt": "2026-01-01T00:00:00Z"
        })];
        if owner_member {
            members.push(json!({
                "id": ACCOUNT_ID, "roleIds": ["r-admin"], "joinedAt": "2026-01-01T00:00:00Z"
            }));
        }
        for space_id in [SPACE_ID, OTHER_SPACE_ID] {
            seed(
                backend,
                "Space",
                json!({
                    "id": space_id,
                    "name": "S",
                    "createdAt": "2026-01-01T00:00:00Z",
                    "memberCount": members.len(),
                    "categories": [],
                    "uncategorizedChannelIds": [],
                    "isPublic": is_public,
                    "isPubliclyPreviewable": false,
                    "roles": [{
                        "id": "r-admin",
                        "name": "Admin",
                        "permissions": ["manage_space", "manage_channels", "view"],
                        "position": 1
                    }],
                    "members": members,
                }),
            )
            .await;
        }
        seed_chat(
            backend,
            "ch1",
            json!({ "kind": "channel", "name": "a", "spaceId": SPACE_ID }),
        )
        .await;
        seed_chat(
            backend,
            "ch2",
            json!({ "kind": "channel", "name": "b", "spaceId": SPACE_ID }),
        )
        .await;
        seed_chat(
            backend,
            "other",
            json!({ "kind": "channel", "name": "c", "spaceId": OTHER_SPACE_ID }),
        )
        .await;
    }

    /// L993: "For channel Chats, a ReadPosition is created when the owner
    /// joins the containing Space." Only that Space's channels get one.
    pub async fn space_join_creates_read_positions_for_its_channels<B: ConformanceBackend>(
        backend: B,
    ) {
        seed_space_with_channels(&backend, true, false).await;
        assert!(positions(&backend).await.is_empty());

        handle_space_join(
            &backend,
            &owner(&backend),
            json!({ "accountId": ACCOUNT_ID, "spaceId": SPACE_ID }),
        )
        .await
        .expect("Space/join");

        assert_eq!(position_chat_ids(&backend).await, vec!["ch1", "ch2"]);
        for p in positions(&backend).await {
            assert!(p.get("lastReadMessageId").is_none(), "{p}");
        }
    }

    /// L993 "destroys it when the Chat is destroyed" holds for a channel
    /// removed by `Space/set` `removeChannels`.
    pub async fn remove_channels_destroys_their_read_positions<B: ConformanceBackend>(backend: B) {
        seed_space_with_channels(&backend, false, true).await;
        seed_position(&backend, "rp1", "ch1").await;
        seed_position(&backend, "rp2", "ch2").await;
        seed_position(&backend, "rp3", "other").await;
        let since = position_state(&backend).await;

        let HandlerOutput { primary: resp, .. } = handle_space_set(
            &backend,
            &owner(&backend),
            &std::collections::BTreeMap::new(),
            json!({
                "accountId": ACCOUNT_ID,
                "update": { SPACE_ID: { "removeChannels": ["ch1"] } }
            }),
            &[],
        )
        .await
        .expect("Space/set");
        assert!(resp["updated"].get(SPACE_ID).is_some(), "{resp}");

        assert_eq!(position_chat_ids(&backend).await, vec!["ch2", "other"]);
        assert_eq!(
            destroyed_positions_since(&backend, &since).await,
            vec![json!("rp1")]
        );
    }

    /// Destroying a Space destroys its channel Chats (§Destroying a Space),
    /// so their ReadPositions go too (L993).
    pub async fn space_destroy_destroys_channel_read_positions<B: ConformanceBackend>(backend: B) {
        seed_space_with_channels(&backend, false, true).await;
        seed_position(&backend, "rp1", "ch1").await;
        seed_position(&backend, "rp2", "ch2").await;
        seed_position(&backend, "rp3", "other").await;
        let since = position_state(&backend).await;

        let HandlerOutput { primary: resp, .. } = handle_space_set(
            &backend,
            &owner(&backend),
            &std::collections::BTreeMap::new(),
            json!({ "accountId": ACCOUNT_ID, "destroy": [SPACE_ID] }),
            &[],
        )
        .await
        .expect("Space/set");
        assert_eq!(resp["destroyed"], json!([SPACE_ID]), "{resp}");

        assert_eq!(position_chat_ids(&backend).await, vec!["other"]);
        let mut destroyed = destroyed_positions_since(&backend, &since).await;
        destroyed.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
        assert_eq!(destroyed, vec![json!("rp1"), json!("rp2")]);
    }

    async fn chat_state<B: ConformanceBackend>(backend: &B) -> String {
        let HandlerOutput { primary: resp, .. } = handle_chat_get(
            backend,
            &owner(backend),
            json!({ "accountId": ACCOUNT_ID, "ids": [] }),
            &[],
        )
        .await
        .expect("Chat/get");
        resp["state"].as_str().expect("state").to_owned()
    }

    async fn chats_updated_since<B: ConformanceBackend>(backend: &B, since: &str) -> Vec<Value> {
        let HandlerOutput { primary: resp, .. } = handle_chat_changes(
            backend,
            &owner(backend),
            json!({ "accountId": ACCOUNT_ID, "sinceState": since }),
            &[],
        )
        .await
        .expect("Chat/changes");
        resp["updated"].as_array().expect("updated").clone()
    }

    async fn message_set<B: ConformanceBackend>(backend: &B, args: Value) -> Value {
        let mut args = args;
        args["accountId"] = json!(ACCOUNT_ID);
        let HandlerOutput { primary: resp, .. } = handle_message_set(
            backend,
            &owner(backend),
            &std::collections::BTreeMap::new(),
            args,
            &[],
        )
        .await
        .expect("Message/set");
        resp
    }

    /// L1434 "updates `Chat.unreadCount` for the affected Chat" changes
    /// Chat data, so (RFC 8620 §5.1) the Chat state moves and (§5.2)
    /// `Chat/changes` lists only the affected Chat.
    ///
    /// Runs only when `maintains_unread_count` is `false`.
    pub async fn read_position_set_advances_chat_state<B: ConformanceBackend>(backend: B) {
        if !exact_unread_count(&backend) {
            return;
        }
        seed_five(&backend).await;
        assert_eq!(unread_of(&backend, "g").await, 5);
        let since = chat_state(&backend).await;

        set_last_read(&backend, "rp-g", json!({ "lastReadMessageId": M3 })).await;

        assert_eq!(unread_of(&backend, "g").await, 2);
        assert_ne!(chat_state(&backend).await, since);
        assert_eq!(
            chats_updated_since(&backend, &since).await,
            vec![json!("g")]
        );
    }

    /// Destroying an unread Message lowers the Chat's `unreadCount`
    /// (L629), so `Chat/changes` reports that Chat (RFC 8620 §5.2).
    ///
    /// Runs only when `maintains_unread_count` is `false`.
    pub async fn message_destroy_advances_chat_state<B: ConformanceBackend>(backend: B) {
        if !exact_unread_count(&backend) {
            return;
        }
        seed_five(&backend).await;
        let since = chat_state(&backend).await;

        let resp = message_set(&backend, json!({ "destroy": [M5] })).await;
        assert_eq!(resp["destroyed"], json!([M5]), "{resp}");

        assert_eq!(unread_of(&backend, "g").await, 4);
        assert_eq!(
            chats_updated_since(&backend, &since).await,
            vec![json!("g")]
        );
    }
}
