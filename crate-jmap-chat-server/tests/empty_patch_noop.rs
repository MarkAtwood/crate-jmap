//! An empty `/set` update patch is a no-op success (bd:JMAP-t8ej.25.5).
//!
//! Oracles:
//!
//! - RFC 8620 §5.3 `update`: "Any server-set properties MAY be included in
//!   the patch if their value is identical to the current server value
//!   (before applying the patches to the object).  Otherwise, the update
//!   MUST be rejected with an "invalidProperties" SetError." and
//!   `updated`: "The value for each id is an object containing any
//!   properties that changed in a way not explicitly requested by the
//!   PatchObject sent to the server, or null if none."
//! - Mark's decision on JMAP-t8ej.25: a patch that is `{}`, literally or
//!   after dropping identical server-set values, succeeds as a no-op:
//!   `updated` maps the id to null, nothing is written, the state is
//!   unchanged; an empty patch of a missing id is `notFound` (§5.3).
//!
//! Each case covers one chat-server `/set` handler that checks server-set
//! and immutable properties: Space, ChatContact, SpaceBan, CustomEmoji,
//! ReadPosition and PresenceStatus.

mod common;

use std::collections::BTreeMap;

use common::{ACCOUNT_ID, MemoryBackend, SPACE_ID};
use jmap_chat_server::{
    handle_ban_get, handle_ban_set, handle_contact_get, handle_contact_set, handle_emoji_get,
    handle_emoji_set, handle_position_get, handle_position_set, handle_presence_get,
    handle_presence_set, handle_space_get, handle_space_set,
};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug)]
enum Kind {
    Space,
    Contact,
    Ban,
    Emoji,
    Position,
    Presence,
}

const KINDS: [Kind; 6] = [
    Kind::Space,
    Kind::Contact,
    Kind::Ban,
    Kind::Emoji,
    Kind::Position,
    Kind::Presence,
];

impl Kind {
    /// The server-set and immutable properties of the type, per
    /// draft-atwood-jmap-chat-00 (jmap-chat-spec 77c3474) §Space,
    /// §ChatContact, §SpaceBan, §CustomEmoji, §ReadPosition and
    /// §PresenceStatus, limited to those the seeded object holds.
    fn read_only_keys(self) -> &'static [&'static str] {
        match self {
            Kind::Space => &[
                "id",
                "roles",
                "members",
                "categories",
                "uncategorizedChannelIds",
            ],
            Kind::Contact => &["id", "identity", "login", "firstSeenAt", "lastSeenAt"],
            Kind::Ban => &["id", "spaceId", "userId", "bannedBy", "createdAt"],
            Kind::Emoji => &["id", "spaceId", "createdBy", "createdAt"],
            Kind::Position => &["id", "chatId"],
            Kind::Presence => &["id", "updatedAt"],
        }
    }
}

/// A backend holding one object of `kind`, and that object's id.
async fn seeded(kind: Kind) -> (MemoryBackend, String) {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    let (ty, id, obj) = match kind {
        Kind::Space => {
            let resp = set(
                &backend,
                kind,
                json!({ "create": { "s0": { "name": "Team" } } }),
            )
            .await;
            let id = resp["created"]["s0"]["id"]
                .as_str()
                .unwrap_or_else(|| panic!("Space not created: {resp}"))
                .to_owned();
            return (backend, id);
        }
        Kind::Contact => (
            "ChatContact",
            "ct1",
            json!({
                "id": "ct1",
                "identity": "alice@example.com",
                "login": "alice@example.com",
                "firstSeenAt": "2026-01-01T00:00:00Z",
                "lastSeenAt": "2026-01-01T00:00:00Z",
                "blocked": false,
            }),
        ),
        Kind::Ban => (
            "SpaceBan",
            "ban1",
            json!({
                "id": "ban1",
                "spaceId": SPACE_ID,
                "userId": "target",
                "bannedBy": ACCOUNT_ID,
                "createdAt": "2026-01-01T00:00:00Z",
            }),
        ),
        Kind::Emoji => (
            "CustomEmoji",
            "emoji1",
            json!({
                "id": "emoji1",
                "spaceId": SPACE_ID,
                "name": "party",
                "blobId": "blob1",
                "createdBy": ACCOUNT_ID,
                "createdAt": "2026-01-01T00:00:00Z",
            }),
        ),
        Kind::Position => (
            "ReadPosition",
            "rp1",
            json!({ "id": "rp1", "chatId": "c1" }),
        ),
        Kind::Presence => (
            "PresenceStatus",
            "ps1",
            json!({
                "id": "ps1",
                "presence": "online",
                "updatedAt": "2026-01-01T00:00:00Z",
            }),
        ),
    };
    backend.insert_object_for_test(ty, ACCOUNT_ID, id, obj);
    (backend, id.to_owned())
}

async fn set(backend: &MemoryBackend, kind: Kind, body: Value) -> Value {
    let mut args = body;
    args["accountId"] = json!(ACCOUNT_ID);
    let refs = BTreeMap::new();
    let out = match kind {
        Kind::Space => handle_space_set(backend, &(), &refs, args, &[]).await,
        Kind::Contact => handle_contact_set(backend, &(), args).await,
        Kind::Ban => handle_ban_set(backend, &(), &refs, args).await,
        Kind::Emoji => handle_emoji_set(backend, &(), &refs, args).await,
        Kind::Position => handle_position_set(backend, &(), &refs, args).await,
        Kind::Presence => handle_presence_set(backend, &(), args).await,
    };
    let HandlerOutput { primary, .. } = out.unwrap_or_else(|e| panic!("{kind:?}/set: {e:?}"));
    primary
}

/// The stored object `id` as `/get` returns it.
async fn stored(backend: &MemoryBackend, kind: Kind, id: &str) -> Value {
    let args = json!({ "accountId": ACCOUNT_ID, "ids": [id] });
    let out = match kind {
        Kind::Space => handle_space_get(backend, &(), args, &[]).await,
        Kind::Contact => handle_contact_get(backend, &(), args).await,
        Kind::Ban => handle_ban_get(backend, &(), args).await,
        Kind::Emoji => handle_emoji_get(backend, &(), args).await,
        Kind::Position => handle_position_get(backend, &(), args).await,
        Kind::Presence => handle_presence_get(backend, &(), args).await,
    };
    let HandlerOutput { primary, .. } = out.unwrap_or_else(|e| panic!("{kind:?}/get: {e:?}"));
    primary["list"][0].clone()
}

/// Send `patch` for the seeded object and assert a no-op success:
/// `updated` maps the id to null, the state and the object are unchanged.
async fn assert_noop(kind: Kind, patch: impl FnOnce(&Value) -> Value) {
    let (backend, id) = seeded(kind).await;
    let before = stored(&backend, kind, &id).await;
    assert!(before.is_object(), "{kind:?}: seeded object readable");
    let resp = set(&backend, kind, json!({ "update": { &id: patch(&before) } })).await;
    assert_eq!(resp["updated"], json!({ &id: null }), "{kind:?}: {resp}");
    assert_eq!(resp["notUpdated"], Value::Null, "{kind:?}: {resp}");
    assert_eq!(resp["newState"], resp["oldState"], "{kind:?}: {resp}");
    assert_eq!(stored(&backend, kind, &id).await, before, "{kind:?}");
}

#[tokio::test]
async fn literal_empty_patch_is_noop_success() {
    for kind in KINDS {
        assert_noop(kind, |_| json!({})).await;
    }
}

#[tokio::test]
async fn identical_server_set_values_only_is_noop_success() {
    for kind in KINDS {
        assert_noop(kind, |before| {
            let patch: serde_json::Map<String, Value> = kind
                .read_only_keys()
                .iter()
                .map(|&k| {
                    let v = before
                        .get(k)
                        .unwrap_or_else(|| panic!("{kind:?}: /get returns {k}: {before}"));
                    (k.to_owned(), v.clone())
                })
                .collect();
            Value::Object(patch)
        })
        .await;
    }
}

/// RFC 8620 §5.3 `notFound`: "The id given to update/destroy cannot be
/// found." An empty patch does not hide a missing object.
#[tokio::test]
async fn empty_patch_of_missing_id_is_not_found() {
    for kind in KINDS {
        let (backend, _) = seeded(kind).await;
        let resp = set(&backend, kind, json!({ "update": { "missing1": {} } })).await;
        assert_eq!(
            resp["notUpdated"]["missing1"]["type"], "notFound",
            "{kind:?}: {resp}"
        );
        assert_eq!(resp["updated"], Value::Null, "{kind:?}: {resp}");
    }
}
