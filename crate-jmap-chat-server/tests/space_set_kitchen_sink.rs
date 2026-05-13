//! Kitchen-sink integration test for `Space/set` (bd:JMAP-g7wu.2.4.6).
//!
//! Exercises all 12 semantic-mutation keys
//! (`addRoles`, `updateRoles`, `removeRoles`, `addMembers`,
//! `updateMembers`, `removeMembers`, `addChannels`, `updateChannels`,
//! `removeChannels`, `addCategories`, `updateCategories`,
//! `removeCategories`) in a single `Space/set` call against a single
//! Space, with an admin caller holding all four permission tags.
//!
//! Goal: verify the 12 variants don't interfere with each other on
//! the existing code paths and that dispatch, parsing, and
//! atomicity hold under a mixed-op load. Per-variant correctness
//! (cascade semantics, hierarchy enforcement, last-admin
//! protection) is covered by the per-op test beads:
//!
//!   - Role/Member ops: tests/role_member_apply.rs (bd:JMAP-g7wu.2.4.3)
//!   - Channel ops: tests/integration.rs space_set_addChannels_* etc.
//!     (bd:JMAP-g7wu.2.4.4)
//!   - Category ops: tests/integration.rs space_set_addCategories_*
//!     etc. (bd:JMAP-g7wu.2.4.5)
//!   - Permission gates: tests/role_member_apply.rs,
//!     tests/channel_category_apply.rs (bd:JMAP-g7wu.2.4.14),
//!     tests/space_metadata_apply.rs (bd:JMAP-g7wu.2.4.13)
//!
//! This test specifically verifies that the orchestration code
//! around those per-op helpers does not break when all 12 keys are
//! present in one wire patch — the dispatch order, the per-family
//! parsing, the single-transaction backend application, and the
//! aggregated change-log emission all hold up.

mod common;

use common::{IdentityBackend, MemoryBackend};
use jmap_chat_server::{handle_space_get, handle_space_set};
use jmap_types::Id;
use serde_json::json;

const ACCOUNT_ID: &str = "a1";
const SPACE_ID: &str = "s1";
const ADMIN_USER: &str = "admin";

/// Seed a Space with the structure needed to exercise every
/// `update*` and `remove*` op without first creating the targets in
/// the same patch. Pre-existing ids:
///
/// - Roles: `r-admin` (the caller's full-permission role at position
///   100), `r-update` (position 30, to be renamed), `r-remove`
///   (position 20, to be removed).
/// - Members: `admin` (the caller), `u-update` (to be renamed via
///   `updateMembers`), `u-remove` (to be removed).
/// - Channels: `ch-update`, `ch-remove`, plus the channels created
///   by `addChannels` in the test patch.
/// - Categories: `cat-update`, `cat-remove`.
///
/// Note: the channels are NOT inserted as separate Chat objects in
/// this seed (no message store wiring). The reference impl's
/// `apply_space_patch` for `removeChannels` cascades to Chat
/// destruction and Message destruction; tests in
/// `tests/integration.rs` cover that cascade. This kitchen-sink
/// test only inspects the Space's wire shape post-patch.
fn seed_kitchen_sink_space(backend: &IdentityBackend) {
    let space_val = json!({
        "id": SPACE_ID,
        "name": "Kitchen Sink Space",
        "description": "pre-patch",
        "createdAt": "2026-01-01T00:00:00Z",
        "memberCount": 3,
        "categories": [
            {
                "id": "cat-update",
                "name": "Category to Update",
                "position": 0,
                "channelIds": []
            },
            {
                "id": "cat-remove",
                "name": "Category to Remove",
                "position": 1,
                "channelIds": []
            }
        ],
        "uncategorizedChannelIds": ["ch-update", "ch-remove"],
        "isPublic": false,
        "isPubliclyPreviewable": false,
        "roles": [
            {
                "id": "r-admin",
                "name": "Admin",
                "permissions": [
                    "manage_space",
                    "manage_roles",
                    "manage_members",
                    "manage_channels"
                ],
                "position": 100
            },
            {
                "id": "r-update",
                "name": "Role to Update",
                "permissions": [],
                "position": 30
            },
            {
                "id": "r-remove",
                "name": "Role to Remove",
                "permissions": [],
                "position": 20
            }
        ],
        "members": [
            {
                "id": ADMIN_USER,
                "roleIds": ["r-admin"],
                "joinedAt": "2026-01-01T00:00:00Z"
            },
            {
                "id": "u-update",
                "roleIds": [],
                "joinedAt": "2026-01-02T00:00:00Z",
                "nick": "OldNick"
            },
            {
                "id": "u-remove",
                "roleIds": [],
                "joinedAt": "2026-01-03T00:00:00Z"
            }
        ],
    });

    backend.inner().register_account(&Id::from(ACCOUNT_ID));
    backend
        .inner()
        .insert_object_for_test("Space", ACCOUNT_ID, SPACE_ID, space_val);

    // Seed corresponding Chat objects for the two pre-existing
    // channels so updateChannels and removeChannels have something
    // to act on. The reference impl's removeChannels cascade
    // reads from the Chat store; missing entries surface as
    // notFound on the per-op outcome.
    //
    // The wire shape carries every required field on `Chat`
    // (draft-atwood-jmap-chat-00 §4.10) so the in-memory store's
    // `Chat/get` deserialization (used downstream by the assertion
    // block) succeeds.
    for (chat_id, name) in [
        ("ch-update", "channel-to-update"),
        ("ch-remove", "channel-to-remove"),
    ] {
        backend.inner().insert_object_for_test(
            "Chat",
            ACCOUNT_ID,
            chat_id,
            json!({
                "id": chat_id,
                "kind": "channel",
                "spaceId": SPACE_ID,
                "name": name,
                "createdAt": "2026-01-01T00:00:00Z",
                "unreadCount": 0,
                "pinnedMessageIds": [],
                "muted": false,
                "receiveTypingIndicators": true
            }),
        );
    }
}

/// Oracle: bd:JMAP-g7wu.2.4.6. A single `Space/set` `update` carrying
/// all 12 semantic-mutation keys against one Space, with an admin
/// caller, must apply atomically and produce no `notUpdated` entry.
///
/// The assertions only verify (a) the wire response shape (entry in
/// `updated`, nothing in `notUpdated`) and (b) the post-patch Space
/// state for each family. Per-op semantics (cascade behavior,
/// hierarchy enforcement, etc.) are covered by the per-op test
/// beads named in the file rustdoc.
#[tokio::test]
async fn space_set_all_twelve_ops_in_one_call() {
    let backend = IdentityBackend::new();
    seed_kitchen_sink_space(&backend);
    let caller = Id::from(ADMIN_USER);

    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": {
                SPACE_ID: {
                    // Role family.
                    "addRoles": [{
                        "id": "placeholder-new-role",
                        "name": "Newly Added",
                        "permissions": [],
                        "position": 10
                    }],
                    "updateRoles": [{
                        "id": "r-update",
                        "name": "Role Renamed In Patch"
                    }],
                    "removeRoles": ["r-remove"],

                    // Member family.
                    "addMembers": [{
                        "id": "u-new",
                        "roleIds": []
                    }],
                    "updateMembers": [{
                        "id": "u-update",
                        "nick": "NewNick"
                    }],
                    "removeMembers": ["u-remove"],

                    // Channel family.
                    "addChannels": [{ "name": "newly-added-channel" }],
                    "updateChannels": [{
                        "id": "ch-update",
                        "name": "channel-renamed-in-patch"
                    }],
                    "removeChannels": ["ch-remove"],

                    // Category family.
                    "addCategories": [{
                        "id": "placeholder-new-cat",
                        "name": "New Category",
                        "position": 2,
                        "channelIds": []
                    }],
                    "updateCategories": [{
                        "id": "cat-update",
                        "name": "Category Renamed In Patch"
                    }],
                    "removeCategories": ["cat-remove"]
                }
            }
        }),
    )
    .await
    .expect("handle_space_set");

    // Wire-shape assertion: no notUpdated entries, one updated entry.
    assert!(
        resp["notUpdated"][SPACE_ID].is_null(),
        "no notUpdated entry expected; full response: {resp:?}"
    );
    // RFC 8620 §5.3: each successful update target produces an
    // entry in `updated`. The reference handler emits a null
    // sentinel when the backend returned `None` (the verbatim-
    // patch case). The presence of the key — not the inner value —
    // is the signal of success.
    assert!(
        resp["updated"]
            .as_object()
            .is_some_and(|m| m.contains_key(SPACE_ID)),
        "updated map must carry the SPACE_ID key (null sentinel allowed): {resp:?}"
    );

    // Read the post-patch Space and verify each family.
    let (get_resp, _) = handle_space_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
    )
    .await
    .expect("Space/get");
    let space = &get_resp["list"][0];

    // ---- Role family ----
    let roles = space["roles"].as_array().expect("roles array");
    // r-remove gone, r-update renamed, r-admin retained, plus one new role.
    assert!(
        !roles.iter().any(|r| r["id"].as_str() == Some("r-remove")),
        "r-remove must be gone post-patch: {roles:?}"
    );
    let updated_role = roles
        .iter()
        .find(|r| r["id"].as_str() == Some("r-update"))
        .expect("r-update must still exist");
    assert_eq!(
        updated_role["name"].as_str().unwrap_or(""),
        "Role Renamed In Patch",
        "r-update must be renamed: {updated_role:?}"
    );
    let new_role_count = roles
        .iter()
        .filter(|r| r["name"].as_str() == Some("Newly Added"))
        .count();
    assert_eq!(
        new_role_count, 1,
        "exactly one new role expected: {roles:?}"
    );

    // ---- Member family ----
    let members = space["members"].as_array().expect("members array");
    // u-remove gone, u-update nick-renamed, admin retained, u-new added.
    assert!(
        !members.iter().any(|m| m["id"].as_str() == Some("u-remove")),
        "u-remove must be gone post-patch: {members:?}"
    );
    let updated_member = members
        .iter()
        .find(|m| m["id"].as_str() == Some("u-update"))
        .expect("u-update must still exist");
    assert_eq!(
        updated_member["nick"].as_str().unwrap_or(""),
        "NewNick",
        "u-update nick must be updated: {updated_member:?}"
    );
    assert!(
        members.iter().any(|m| m["id"].as_str() == Some("u-new")),
        "u-new must have been added: {members:?}"
    );

    // ---- Channel family (channels live as Space.uncategorizedChannelIds
    //      plus inside category.channelIds; combine both sources) ----
    let mut all_channel_ids: Vec<String> = space["uncategorizedChannelIds"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    if let Some(cats) = space["categories"].as_array() {
        for cat in cats {
            if let Some(ids) = cat["channelIds"].as_array() {
                for id in ids {
                    if let Some(s) = id.as_str() {
                        all_channel_ids.push(s.to_owned());
                    }
                }
            }
        }
    }
    assert!(
        !all_channel_ids.iter().any(|id| id == "ch-remove"),
        "ch-remove must be gone post-patch: {all_channel_ids:?}"
    );
    assert!(
        all_channel_ids.iter().any(|id| id == "ch-update"),
        "ch-update must still exist: {all_channel_ids:?}"
    );
    // ch-update was renamed; verify via the Chat store.
    let (chat_get, _) = jmap_chat_server::handle_chat_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": ["ch-update"] }),
    )
    .await
    .expect("Chat/get");
    assert_eq!(
        chat_get["list"][0]["name"].as_str().unwrap_or(""),
        "channel-renamed-in-patch",
        "ch-update must be renamed: {chat_get:?}"
    );
    // The new channel exists. Reference impl assigns sequential ids.
    let new_channel_count = all_channel_ids.len();
    // Started with 2 channels (ch-update, ch-remove), removed 1,
    // added 1 → 2 channels post-patch.
    assert_eq!(
        new_channel_count, 2,
        "channel count must be (2 - 1 + 1 = 2) post-patch: {all_channel_ids:?}"
    );

    // ---- Category family ----
    let cats = space["categories"].as_array().expect("categories array");
    assert!(
        !cats.iter().any(|c| c["id"].as_str() == Some("cat-remove")),
        "cat-remove must be gone post-patch: {cats:?}"
    );
    let updated_cat = cats
        .iter()
        .find(|c| c["id"].as_str() == Some("cat-update"))
        .expect("cat-update must still exist");
    assert_eq!(
        updated_cat["name"].as_str().unwrap_or(""),
        "Category Renamed In Patch",
        "cat-update must be renamed: {updated_cat:?}"
    );
    let new_cat_count = cats
        .iter()
        .filter(|c| c["name"].as_str() == Some("New Category"))
        .count();
    assert_eq!(
        new_cat_count, 1,
        "exactly one new category expected: {cats:?}"
    );
}

/// Oracle: bd:JMAP-g7wu.2.4.6 (corollary). The same kitchen-sink
/// patch run under single-user mode (`MemoryBackend` with
/// `CallerCtx = ()`, `principal_id = None`) also succeeds — the
/// gate logic short-circuits and every op applies. This serves as
/// a regression bound: the prior pre-`.4.13`/`.4.14` behavior was
/// "identity-less callers can do anything"; we preserve that
/// posture in single-user mode.
#[tokio::test]
async fn space_set_all_twelve_ops_single_user_mode() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend.insert_object_for_test(
        "Space",
        ACCOUNT_ID,
        SPACE_ID,
        json!({
            "id": SPACE_ID,
            "name": "Kitchen Sink Single-User",
            "description": "pre-patch",
            "createdAt": "2026-01-01T00:00:00Z",
            "memberCount": 1,
            "categories": [
                { "id": "cat-update", "name": "Cat U", "position": 0, "channelIds": [] },
                { "id": "cat-remove", "name": "Cat R", "position": 1, "channelIds": [] }
            ],
            "uncategorizedChannelIds": ["ch-update", "ch-remove"],
            "isPublic": false,
            "isPubliclyPreviewable": false,
            "roles": [
                { "id": "r-update", "name": "RU", "permissions": [], "position": 30 },
                { "id": "r-remove", "name": "RR", "permissions": [], "position": 20 }
            ],
            "members": [
                { "id": "u-update", "roleIds": [], "joinedAt": "2026-01-01T00:00:00Z" },
                { "id": "u-remove", "roleIds": [], "joinedAt": "2026-01-01T00:00:00Z" }
            ]
        }),
    );
    for (chat_id, name) in [("ch-update", "u"), ("ch-remove", "r")] {
        backend.insert_object_for_test(
            "Chat",
            ACCOUNT_ID,
            chat_id,
            json!({
                "id": chat_id,
                "kind": "channel",
                "spaceId": SPACE_ID,
                "name": name,
                "createdAt": "2026-01-01T00:00:00Z",
                "unreadCount": 0,
                "pinnedMessageIds": [],
                "muted": false,
                "receiveTypingIndicators": true
            }),
        );
    }

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": {
                SPACE_ID: {
                    "addRoles": [{ "id": "p", "name": "X", "permissions": [], "position": 10 }],
                    "updateRoles": [{ "id": "r-update", "name": "RUx" }],
                    "removeRoles": ["r-remove"],
                    "addMembers": [{ "id": "u-new", "roleIds": [] }],
                    "updateMembers": [{ "id": "u-update", "nick": "n" }],
                    "removeMembers": ["u-remove"],
                    "addChannels": [{ "name": "nc" }],
                    "updateChannels": [{ "id": "ch-update", "name": "u2" }],
                    "removeChannels": ["ch-remove"],
                    "addCategories": [{ "id": "p", "name": "C", "position": 2, "channelIds": [] }],
                    "updateCategories": [{ "id": "cat-update", "name": "CUx" }],
                    "removeCategories": ["cat-remove"]
                }
            }
        }),
    )
    .await
    .expect("handle_space_set");

    assert!(
        resp["notUpdated"][SPACE_ID].is_null(),
        "single-user mode must succeed on all 12 ops: {resp:?}"
    );
}
