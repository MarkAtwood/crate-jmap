//! `SpacePatchOp` and per-collection `*Patch` types for `Space/set` update operations
//! (draft-atwood-jmap-chat-00 §Space/set).
//!
//! `Space/set` update operations use semantic mutation keys
//! (`addRoles`, `removeRoles`, `updateRoles`, `addMembers`, `removeMembers`,
//! `updateMembers`, `addChannels`, `removeChannels`, `updateChannels`,
//! `addCategories`, `removeCategories`, `updateCategories`) rather than RFC 8620
//! JSON Pointer patches. Each key names one permission-checked,
//! cascade-sensitive mutation on an ordered, server-enforced collection.
//!
//! Server-side handlers parse the wire JSON patch object, then unfold each
//! pluralized array entry into a single [`SpacePatchOp`] variant. The handler
//! applies them in a defined order with the appropriate permission checks
//! per `draft-atwood-jmap-chat-00 §Space/set`.
//!
//! This module defines:
//!
//! - [`SpacePatchOp`] — twelve variants, one per wire key entry.
//! - [`RolePatch`], [`MemberPatch`], [`ChannelPatch`], [`CategoryPatch`] —
//!   per-field optional updates for the three `update*` operations. Nullable
//!   fields use [`Clearable<T>`] to distinguish `null` (clear) from absence
//!   (unchanged).
//! - [`ChannelCreate`] — the per-entry input for `addChannels`, which mirrors
//!   the spec's allowed-fields subset (`name`, optional `categoryId`,
//!   `position`, `topic`). The server creates a full [`crate::Chat`] of
//!   `kind: "channel"` with the new id and `spaceId` set.
//!
//! Construction is the consumer's responsibility (no `::new()` methods); use
//! `serde_json::from_value`/`from_str` or struct-literal syntax.

use jmap_types::Id;
use serde::{Deserialize, Serialize};

use crate::chat::ChannelPermission;
use crate::clearable::{some_clearable, Clearable};
use crate::space::{Category, SpaceRole};

/// Per-entry input for `addChannels` (draft-atwood-jmap-chat-00 §Space/set).
///
/// The server creates a [`crate::Chat`] record of `kind: "channel"` with the
/// new id and `spaceId` set to the host Space's id. This struct carries only
/// the client-supplied subset.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelCreate {
    /// The `name` property (draft-atwood-jmap-chat-00 §Space/set — required).
    pub name: String,
    /// The `categoryId` property (draft-atwood-jmap-chat-00 §Space/set — optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_id: Option<Id>,
    /// The `position` property (draft-atwood-jmap-chat-00 §Space/set — optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<u64>,
    /// The `topic` property (draft-atwood-jmap-chat-00 §Space/set — optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A per-entry patch for `updateRoles` (draft-atwood-jmap-chat-00 §Space/set).
///
/// Each field is `Option<_>`; an absent field leaves the property unchanged.
/// The `color` field is `Option<Clearable<String>>` because `SpaceRole.color`
/// is itself optional: a wire `null` means "clear the color" while absence
/// means "leave the existing color alone".
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RolePatch {
    /// The `name` property (draft-atwood-jmap-chat-00 §Space/set updateRoles).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The `color` property (draft-atwood-jmap-chat-00 §Space/set updateRoles).
    ///
    /// `null` clears the color; absent leaves it unchanged.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "some_clearable"
    )]
    pub color: Option<Clearable<String>>,
    /// The `permissions` property (draft-atwood-jmap-chat-00 §Space/set updateRoles).
    ///
    /// Drawn from the spec-enumerated permission vocabulary
    /// exported as [`crate::vocabulary::SPEC_PERMISSION_NAMES`].
    /// Same forward-compat contract as
    /// [`SpaceRole::permissions`](crate::SpaceRole::permissions):
    /// servers MUST ignore unrecognized names; consumers SHOULD
    /// validate caller-supplied input against the const list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Vec<String>>,
    /// The `position` property (draft-atwood-jmap-chat-00 §Space/set updateRoles).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<u64>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A per-entry patch for `updateMembers` (draft-atwood-jmap-chat-00 §Space/set).
///
/// The `nick` field is `Option<Clearable<String>>` because `SpaceMember.nick`
/// is itself optional: a wire `null` means "clear the nick" while absence
/// means "leave the existing nick alone".
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberPatch {
    /// The `roleIds` property (draft-atwood-jmap-chat-00 §Space/set updateMembers).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role_ids: Option<Vec<Id>>,
    /// The `nick` property (draft-atwood-jmap-chat-00 §Space/set updateMembers).
    ///
    /// `null` clears the nick; absent leaves it unchanged.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "some_clearable"
    )]
    pub nick: Option<Clearable<String>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A per-entry patch for `updateChannels` (draft-atwood-jmap-chat-00 §Space/set).
///
/// The `categoryId` field is `Option<Clearable<Id>>` because `Chat.categoryId`
/// is itself optional: a wire `null` means "remove from category (move to
/// uncategorizedChannelIds)" while absence means "leave the assignment alone".
///
/// `permissionOverrides` is wholesale-replaced when present; absence leaves
/// the existing overrides untouched. Use `null` (clearable) if you want to
/// drop every override.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPatch {
    /// The `name` property (draft-atwood-jmap-chat-00 §Space/set updateChannels).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The `topic` property (draft-atwood-jmap-chat-00 §Space/set updateChannels).
    ///
    /// `null` clears the topic; absent leaves it unchanged.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "some_clearable"
    )]
    pub topic: Option<Clearable<String>>,
    /// The `categoryId` property (draft-atwood-jmap-chat-00 §Space/set updateChannels).
    ///
    /// `null` moves the channel to `uncategorizedChannelIds`; absent leaves
    /// the assignment unchanged.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "some_clearable"
    )]
    pub category_id: Option<Clearable<Id>>,
    /// The `position` property (draft-atwood-jmap-chat-00 §Space/set updateChannels).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<u64>,
    /// The `slowModeSeconds` property (draft-atwood-jmap-chat-00 §Space/set updateChannels).
    ///
    /// `null` clears the per-channel slow-mode override; absent leaves it
    /// unchanged.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "some_clearable"
    )]
    pub slow_mode_seconds: Option<Clearable<u64>>,
    /// The `permissionOverrides` property (draft-atwood-jmap-chat-00 §Space/set updateChannels).
    ///
    /// `null` clears every override; absent leaves the existing overrides
    /// unchanged.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "some_clearable"
    )]
    pub permission_overrides: Option<Clearable<Vec<ChannelPermission>>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A per-entry patch for `updateCategories` (draft-atwood-jmap-chat-00 §Space/set).
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryPatch {
    /// The `name` property (draft-atwood-jmap-chat-00 §Space/set updateCategories).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The `position` property (draft-atwood-jmap-chat-00 §Space/set updateCategories).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<u64>,
    /// The `channelIds` property (draft-atwood-jmap-chat-00 §Space/set updateCategories).
    ///
    /// Wholesale-replaces the category's ordered channel list when present;
    /// absent leaves it unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_ids: Option<Vec<Id>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A single Space/set update mutation operation.
///
/// Server-side handlers parse the wire JSON patch object (which has 12
/// pluralized keys), then unfold each array entry into a separate
/// `SpacePatchOp` value. Each variant corresponds to one entry of one
/// wire key:
///
/// | Wire key (draft-atwood-jmap-chat-00 §Space/set) | Variant |
/// |---|---|
/// | `addRoles` entry        | [`SpacePatchOp::AddRole`] |
/// | `removeRoles` entry     | [`SpacePatchOp::RemoveRole`] |
/// | `updateRoles` entry     | [`SpacePatchOp::UpdateRole`] |
/// | `addMembers` entry      | [`SpacePatchOp::AddMember`] |
/// | `removeMembers` entry   | [`SpacePatchOp::RemoveMember`] |
/// | `updateMembers` entry   | [`SpacePatchOp::UpdateMember`] |
/// | `addChannels` entry     | [`SpacePatchOp::AddChannel`] |
/// | `removeChannels` entry  | [`SpacePatchOp::RemoveChannel`] |
/// | `updateChannels` entry  | [`SpacePatchOp::UpdateChannel`] |
/// | `addCategories` entry   | [`SpacePatchOp::AddCategory`] |
/// | `removeCategories` entry| [`SpacePatchOp::RemoveCategory`] |
/// | `updateCategories` entry| [`SpacePatchOp::UpdateCategory`] |
///
/// This is a Rust internal representation; it has no wire form of its own.
/// The wire form is the pluralized patch object as defined in the draft.
/// Round-trip JSON tests exist for the contained payload types
/// ([`SpaceRole`], [`Category`], [`ChannelCreate`], [`RolePatch`],
/// [`MemberPatch`], [`ChannelPatch`], [`CategoryPatch`]) — see this module's
/// test section.
///
/// For `Add*` variants whose payload is a server-stored type with a
/// server-assigned `id` ([`SpaceRole`], [`Category`]), the input value
/// carries a placeholder `id`; the server replaces it with a real id
/// before storing.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum SpacePatchOp {
    /// Add a new role to the Space (one entry from wire key `addRoles`).
    ///
    /// The contained [`SpaceRole`]'s `id` is a placeholder; the server
    /// replaces it with a ULID before storing.
    AddRole(SpaceRole),
    /// Remove a role from the Space (one entry from wire key `removeRoles`).
    ///
    /// Members holding only the removed role are demoted to `@everyone`.
    RemoveRole(Id),
    /// Update an existing role (one entry from wire key `updateRoles`).
    UpdateRole { id: Id, patch: RolePatch },
    /// Add a member to the Space (one entry from wire key `addMembers`).
    ///
    /// `user_id` is the [`crate::ChatContact`] id; `role_ids` may be empty
    /// (the member gets only `@everyone`).
    AddMember { user_id: Id, role_ids: Vec<Id> },
    /// Remove a member from the Space (one entry from wire key `removeMembers`).
    ///
    /// The owner cannot be removed.
    RemoveMember(Id),
    /// Update an existing member (one entry from wire key `updateMembers`).
    UpdateMember { user_id: Id, patch: MemberPatch },
    /// Add a channel to the Space (one entry from wire key `addChannels`).
    ///
    /// The server creates a [`crate::Chat`] of `kind: "channel"` with the
    /// new id and `spaceId` set.
    AddChannel(ChannelCreate),
    /// Remove a channel from the Space (one entry from wire key `removeChannels`).
    ///
    /// Cascades to all Messages in the channel.
    RemoveChannel(Id),
    /// Update an existing channel (one entry from wire key `updateChannels`).
    UpdateChannel { id: Id, patch: ChannelPatch },
    /// Add a category to the Space (one entry from wire key `addCategories`).
    ///
    /// The contained [`Category`]'s `id` is a placeholder; the server
    /// replaces it with a ULID before storing.
    AddCategory(Category),
    /// Remove a category from the Space (one entry from wire key `removeCategories`).
    ///
    /// Channels in the removed category move to `uncategorizedChannelIds`.
    RemoveCategory(Id),
    /// Update an existing category (one entry from wire key `updateCategories`).
    UpdateCategory { id: Id, patch: CategoryPatch },
}

#[cfg(test)]
mod tests {
    use super::*;

    // Oracle: spec §Space/set updateRoles — role patch with `color: null` clears
    // the optional color field while leaving other absent fields unchanged.
    #[test]
    fn role_patch_color_null_clears() {
        let json = r#"{
            "name": "Moderator",
            "color": null
        }"#;
        let p: RolePatch = serde_json::from_str(json).expect("deserialize RolePatch");
        assert_eq!(p.name.as_deref(), Some("Moderator"));
        assert_eq!(p.color, Some(Clearable::Clear));
        assert_eq!(p.permissions, None);
        assert_eq!(p.position, None);
    }

    // Oracle: spec §Space/set updateRoles — role patch with a colored value sets it.
    #[test]
    fn role_patch_color_set() {
        let json = r##"{"color": "#ff00ff"}"##;
        let p: RolePatch = serde_json::from_str(json).expect("deserialize RolePatch");
        assert_eq!(p.color, Some(Clearable::Set("#ff00ff".to_owned())));
    }

    // Oracle: spec §Space/set updateRoles — absent color stays None (not Clear).
    #[test]
    fn role_patch_color_absent_is_none() {
        let json = r#"{"name": "Foo"}"#;
        let p: RolePatch = serde_json::from_str(json).expect("deserialize RolePatch");
        assert_eq!(p.color, None);
    }

    // Oracle: spec §Space/set updateRoles — round-trip preserves all fields.
    #[test]
    fn role_patch_roundtrip() {
        let json = r##"{"name":"Mod","color":"#00ff00","permissions":["chat:read","chat:write"],"position":5}"##;
        let p: RolePatch = serde_json::from_str(json).expect("deserialize RolePatch");
        let re = serde_json::to_string(&p).expect("serialize RolePatch");
        let p2: RolePatch = serde_json::from_str(&re).expect("re-deserialize RolePatch");
        assert_eq!(p, p2);
        assert_eq!(p.permissions.as_ref().map(Vec::len), Some(2));
        assert_eq!(p.position, Some(5));
    }

    // Oracle: spec §Space/set updateMembers — nick clearable, role_ids replaceable.
    #[test]
    fn member_patch_roundtrip() {
        let json = r#"{"roleIds":["r1","r2"],"nick":"Captain"}"#;
        let p: MemberPatch = serde_json::from_str(json).expect("deserialize MemberPatch");
        let re = serde_json::to_string(&p).expect("serialize MemberPatch");
        let p2: MemberPatch = serde_json::from_str(&re).expect("re-deserialize MemberPatch");
        assert_eq!(p, p2);
        assert_eq!(p.nick, Some(Clearable::Set("Captain".to_owned())));
        assert_eq!(p.role_ids.as_ref().map(Vec::len), Some(2));
    }

    // Oracle: spec §Space/set updateMembers — nick:null clears.
    #[test]
    fn member_patch_nick_clear() {
        let json = r#"{"nick":null}"#;
        let p: MemberPatch = serde_json::from_str(json).expect("deserialize MemberPatch");
        assert_eq!(p.nick, Some(Clearable::Clear));
        assert_eq!(p.role_ids, None);
    }

    // Oracle: spec §Space/set updateChannels — categoryId:null moves to
    // uncategorized, while categoryId:"cat-2" reassigns.
    #[test]
    fn channel_patch_category_id_clearable() {
        let null_json = r#"{"categoryId":null}"#;
        let p: ChannelPatch =
            serde_json::from_str(null_json).expect("deserialize ChannelPatch (null)");
        assert_eq!(p.category_id, Some(Clearable::Clear));

        let set_json = r#"{"categoryId":"cat-2"}"#;
        let p: ChannelPatch =
            serde_json::from_str(set_json).expect("deserialize ChannelPatch (set)");
        assert_eq!(p.category_id, Some(Clearable::Set(Id::from("cat-2"))));
    }

    // Oracle: spec §Space/set updateChannels — full round-trip preserves every
    // patchable property.
    #[test]
    fn channel_patch_roundtrip_full() {
        let json = r#"{
            "name": "general-2",
            "topic": "Daily updates",
            "categoryId": "cat-1",
            "position": 0,
            "slowModeSeconds": 30,
            "permissionOverrides": [
                {"targetId":"r1","targetType":"role","allow":["chat:read"],"deny":[]}
            ]
        }"#;
        let p: ChannelPatch = serde_json::from_str(json).expect("deserialize ChannelPatch");
        let re = serde_json::to_string(&p).expect("serialize ChannelPatch");
        let p2: ChannelPatch = serde_json::from_str(&re).expect("re-deserialize ChannelPatch");
        assert_eq!(p, p2);
        assert_eq!(p.name.as_deref(), Some("general-2"));
        assert_eq!(p.position, Some(0));
        assert!(matches!(p.permission_overrides, Some(Clearable::Set(_))));
    }

    // Oracle: spec §Space/set updateCategories — patch round-trip.
    #[test]
    fn category_patch_roundtrip() {
        let json = r#"{"name":"Voice","position":2,"channelIds":["ch1","ch2"]}"#;
        let p: CategoryPatch = serde_json::from_str(json).expect("deserialize CategoryPatch");
        let re = serde_json::to_string(&p).expect("serialize CategoryPatch");
        let p2: CategoryPatch = serde_json::from_str(&re).expect("re-deserialize CategoryPatch");
        assert_eq!(p, p2);
        assert_eq!(p.channel_ids.as_ref().map(Vec::len), Some(2));
    }

    // Oracle: spec §Space/set addChannels — entry fields and round-trip.
    #[test]
    fn channel_create_roundtrip() {
        let json =
            r#"{"name":"new-channel","categoryId":"cat-3","position":7,"topic":"Discussion"}"#;
        let c: ChannelCreate = serde_json::from_str(json).expect("deserialize ChannelCreate");
        let re = serde_json::to_string(&c).expect("serialize ChannelCreate");
        let c2: ChannelCreate = serde_json::from_str(&re).expect("re-deserialize ChannelCreate");
        assert_eq!(c, c2);
        assert_eq!(c.name, "new-channel");
        assert_eq!(c.category_id.as_ref(), Some(&Id::from("cat-3")));
    }

    // Oracle: spec §Space/set addChannels — name is the only required field.
    #[test]
    fn channel_create_minimal() {
        let json = r#"{"name":"basic"}"#;
        let c: ChannelCreate = serde_json::from_str(json).expect("deserialize ChannelCreate");
        assert_eq!(c.name, "basic");
        assert_eq!(c.category_id, None);
        assert_eq!(c.position, None);
        assert_eq!(c.topic, None);
    }

    // Compile-only: SpacePatchOp enum has all 12 variants and constructs cleanly.
    #[test]
    fn space_patch_op_variant_construction() {
        let role = SpaceRole {
            id: Id::from("placeholder"),
            name: "Test".to_owned(),
            color: None,
            permissions: vec!["chat:read".to_owned()],
            position: 1,
            extra: serde_json::Map::new(),
        };
        let cat = Category {
            id: Id::from("placeholder"),
            name: "Voice".to_owned(),
            position: 0,
            channel_ids: vec![],
            extra: serde_json::Map::new(),
        };

        let ops: Vec<SpacePatchOp> = vec![
            SpacePatchOp::AddRole(role.clone()),
            SpacePatchOp::RemoveRole(Id::from("r1")),
            SpacePatchOp::UpdateRole {
                id: Id::from("r1"),
                patch: RolePatch::default(),
            },
            SpacePatchOp::AddMember {
                user_id: Id::from("u1"),
                role_ids: vec![Id::from("r1")],
            },
            SpacePatchOp::RemoveMember(Id::from("u1")),
            SpacePatchOp::UpdateMember {
                user_id: Id::from("u1"),
                patch: MemberPatch::default(),
            },
            SpacePatchOp::AddChannel(ChannelCreate {
                name: "ch".to_owned(),
                category_id: None,
                position: None,
                topic: None,
                extra: serde_json::Map::new(),
            }),
            SpacePatchOp::RemoveChannel(Id::from("ch1")),
            SpacePatchOp::UpdateChannel {
                id: Id::from("ch1"),
                patch: ChannelPatch::default(),
            },
            SpacePatchOp::AddCategory(cat),
            SpacePatchOp::RemoveCategory(Id::from("cat1")),
            SpacePatchOp::UpdateCategory {
                id: Id::from("cat1"),
                patch: CategoryPatch::default(),
            },
        ];
        assert_eq!(ops.len(), 12);
    }

    // ── Extras-preservation policy tests (JMAP-lbdy.3) ───────────────────

    /// `ChannelCreate.extra` captures vendor fields and preserves them.
    #[test]
    fn channel_create_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "name": "general",
            "acmeCorpDefaultRetentionDays": 30
        });
        let cc: ChannelCreate = serde_json::from_value(raw).unwrap();
        assert_eq!(
            cc.extra
                .get("acmeCorpDefaultRetentionDays")
                .and_then(|v| v.as_u64()),
            Some(30)
        );
        let back = serde_json::to_value(&cc).unwrap();
        assert_eq!(back["acmeCorpDefaultRetentionDays"], 30);
    }

    /// `RolePatch.extra` captures vendor fields and preserves them.
    #[test]
    fn role_patch_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "name": "new-name",
            "acmeCorpPatchOrigin": "admin-console"
        });
        let p: RolePatch = serde_json::from_value(raw).unwrap();
        assert_eq!(
            p.extra.get("acmeCorpPatchOrigin").and_then(|v| v.as_str()),
            Some("admin-console")
        );
        let back = serde_json::to_value(&p).unwrap();
        assert_eq!(back["acmeCorpPatchOrigin"], "admin-console");
    }

    /// `MemberPatch.extra` captures vendor fields and preserves them.
    #[test]
    fn member_patch_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "nick": "alice",
            "acmeCorpReviewerId": "mod-7"
        });
        let p: MemberPatch = serde_json::from_value(raw).unwrap();
        assert_eq!(
            p.extra.get("acmeCorpReviewerId").and_then(|v| v.as_str()),
            Some("mod-7")
        );
        let back = serde_json::to_value(&p).unwrap();
        assert_eq!(back["acmeCorpReviewerId"], "mod-7");
    }

    /// `ChannelPatch.extra` captures vendor fields and preserves them.
    #[test]
    fn channel_patch_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "name": "ch-new",
            "acmeCorpAutoArchive": true
        });
        let p: ChannelPatch = serde_json::from_value(raw).unwrap();
        assert_eq!(
            p.extra.get("acmeCorpAutoArchive").and_then(|v| v.as_bool()),
            Some(true)
        );
        let back = serde_json::to_value(&p).unwrap();
        assert_eq!(back["acmeCorpAutoArchive"], true);
    }

    /// `CategoryPatch.extra` captures vendor fields and preserves them.
    #[test]
    fn category_patch_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "name": "cat-new",
            "acmeCorpCollapsed": false
        });
        let p: CategoryPatch = serde_json::from_value(raw).unwrap();
        assert_eq!(
            p.extra.get("acmeCorpCollapsed").and_then(|v| v.as_bool()),
            Some(false)
        );
        let back = serde_json::to_value(&p).unwrap();
        assert_eq!(back["acmeCorpCollapsed"], false);
    }
}
