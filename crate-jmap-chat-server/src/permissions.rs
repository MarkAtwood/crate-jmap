//! Pure permission-vocabulary helpers for backend authorization gating.
//!
//! This module hosts pure (no I/O, no state) mappings from server-side
//! mutation values onto the permission-string vocabulary defined by
//! draft-atwood-jmap-chat-00 §Space/set. Backend implementations of
//! [`ChatBackend::apply_space_patch`] consume these helpers to gate
//! per-op authorization after resolving the caller's effective
//! permission set in a Space.
//!
//! Per workspace AGENTS.md "backend canonical" policy, handlers MUST NOT
//! call these helpers for permission gating — only backends do.
//!
//! [`ChatBackend::apply_space_patch`]: crate::backend::ChatBackend::apply_space_patch

use crate::backend::SpacePatchOp;

/// Permission string: top-level Space metadata mutations.
///
/// Per draft-atwood-jmap-chat-00 §Space/set lines 1093, 1096 (top-level
/// metadata: `name`, `description`, `iconBlobId`, `isPublic`,
/// `isPubliclyPreviewable`).
pub const MANAGE_SPACE: &str = "manage_space";

/// Permission string: role-set mutations.
///
/// Per draft-atwood-jmap-chat-00 §Space/set lines 1102, 1105, 1108
/// (`addRoles`, `removeRoles`, `updateRoles`).
pub const MANAGE_ROLES: &str = "manage_roles";

/// Permission string: member-set mutations.
///
/// Per draft-atwood-jmap-chat-00 §Space/set lines 1111, 1114, 1117
/// (`addMembers`, `removeMembers`, `updateMembers`).
pub const MANAGE_MEMBERS: &str = "manage_members";

/// Permission string: channel- and category-set mutations.
///
/// Per draft-atwood-jmap-chat-00 §Space/set lines 1120, 1123, 1126, 1129
/// (`addChannels`, `removeChannels`, `updateChannels`, `addCategories`,
/// `removeCategories`, `updateCategories`).
pub const MANAGE_CHANNELS: &str = "manage_channels";

/// Sentinel returned for `SpacePatchOp` variants the helper does not
/// recognize. [`SpacePatchOp`] is `#[non_exhaustive]` upstream, so a
/// future-added variant cannot be matched here at compile time.
///
/// Returning a non-empty permission slice containing a string no realistic
/// caller will ever hold ensures the helper fails closed: an unrecognized
/// op cannot be authorized merely by absence of a permission requirement.
/// Backends that observe this sentinel MUST reject the op.
const UNKNOWN_OP_SENTINEL: &str = "__unknown_space_patch_op__";

/// Returns the set of permission strings required to apply this
/// [`SpacePatchOp`] variant, per draft-atwood-jmap-chat-00 §Space/set
/// lines 1093, 1096, 1102, 1105, 1108, 1111, 1114, 1117, 1120, 1123,
/// 1126, 1129.
///
/// Permissions enumerated by the draft:
///
/// - `manage_space` — top-level metadata (`name`, `description`,
///   `iconBlobId`, `isPublic`, `isPubliclyPreviewable`). Top-level
///   metadata is patched via the standard JMAP [`PatchObject`] wire
///   shape, not via [`SpacePatchOp`] values, so no variant here returns
///   `manage_space`; backends must apply this gate themselves to the
///   top-level wire-keys before unfolding the structural `add*` /
///   `remove*` / `update*` arrays into ops.
/// - `manage_roles` — `addRoles`, `removeRoles`, `updateRoles`. Also
///   required (in addition to `manage_members`) for `updateMembers`
///   entries that modify `roleIds`.
/// - `manage_members` — `addMembers`, `removeMembers`, `updateMembers`.
/// - `manage_channels` — `addChannels`, `removeChannels`,
///   `updateChannels`, `addCategories`, `removeCategories`,
///   `updateCategories`.
///
/// This helper is pure and has no side effects. Backends consume it
/// inside [`ChatBackend::apply_space_patch`] after resolving the
/// caller's effective permissions. Handlers MUST NOT consume this
/// helper for permission gating — gates are backend-canonical per
/// workspace AGENTS.md.
///
/// [`PatchObject`]: jmap_types::PatchObject
/// [`ChatBackend::apply_space_patch`]: crate::backend::ChatBackend::apply_space_patch
pub fn required_permissions_for_op(op: &SpacePatchOp) -> &'static [&'static str] {
    match op {
        // Role family — draft §Space/set lines 1102, 1105, 1108.
        SpacePatchOp::AddRole(_)
        | SpacePatchOp::RemoveRole(_)
        | SpacePatchOp::UpdateRole { .. } => &[MANAGE_ROLES],

        // Member add/remove — draft §Space/set lines 1111, 1114.
        SpacePatchOp::AddMember { .. } | SpacePatchOp::RemoveMember(_) => &[MANAGE_MEMBERS],

        // Member update — draft §Space/set line 1117. Modifying `roleIds`
        // requires `manage_roles` in addition to `manage_members`; nick-only
        // edits require only `manage_members`.
        SpacePatchOp::UpdateMember { patch, .. } => {
            if patch.role_ids.is_some() {
                &[MANAGE_MEMBERS, MANAGE_ROLES]
            } else {
                &[MANAGE_MEMBERS]
            }
        }

        // Channel family — draft §Space/set lines 1120, 1123, 1126.
        SpacePatchOp::AddChannel(_)
        | SpacePatchOp::RemoveChannel(_)
        | SpacePatchOp::UpdateChannel { .. } => &[MANAGE_CHANNELS],

        // Category family — draft §Space/set line 1129. Categories are a
        // sub-property of the channel-set per the draft; the same permission
        // gates both.
        SpacePatchOp::AddCategory(_)
        | SpacePatchOp::RemoveCategory(_)
        | SpacePatchOp::UpdateCategory { .. } => &[MANAGE_CHANNELS],

        // `SpacePatchOp` is `#[non_exhaustive]` upstream. A future-added
        // variant cannot be matched at compile time from this downstream
        // crate. Fail closed by returning a sentinel string no caller can
        // realistically hold; backends MUST reject ops whose required-set
        // contains this sentinel.
        _ => &[UNKNOWN_OP_SENTINEL],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jmap_chat_types::space_set::{
        CategoryPatch, ChannelCreate, ChannelPatch, MemberPatch, RolePatch,
    };
    use jmap_chat_types::{Category, Clearable, SpaceRole};
    use jmap_types::Id;

    // Oracle for the spec-line → permission mapping: draft-atwood-jmap-chat-00
    // §Space/set lines 1093, 1096, 1102, 1105, 1108, 1111, 1114, 1117, 1120,
    // 1123, 1126, 1129. Each test constructs the SpacePatchOp variant from a
    // hand-built payload (deserialized from spec-shaped JSON for types without
    // `Default`, or built via `Default::default()` for the patch types) and
    // asserts the slice returned by the helper matches the table in the
    // rustdoc.
    //
    // `SpaceRole`, `Category`, and `ChannelCreate` are `#[non_exhaustive]`
    // without `Default`, so they are constructed via `serde_json::from_value`
    // from hand-written JSON shaped after draft-atwood-jmap-chat-00 §4.12,
    // §4.14, and §Space/set respectively. This satisfies the workspace
    // independent-oracle rule: the JSON is hand-typed from the spec, not
    // derived from the code under test.

    fn role_fixture() -> SpaceRole {
        serde_json::from_value(serde_json::json!({
            "id": "r1",
            "name": "Moderator",
            "permissions": ["chat:read"],
            "position": 0
        }))
        .expect("role fixture must deserialize")
    }

    fn category_fixture() -> Category {
        serde_json::from_value(serde_json::json!({
            "id": "cat1",
            "name": "Voice",
            "position": 0,
            "channelIds": []
        }))
        .expect("category fixture must deserialize")
    }

    fn channel_create_fixture() -> ChannelCreate {
        serde_json::from_value(serde_json::json!({
            "name": "general"
        }))
        .expect("channelCreate fixture must deserialize")
    }

    #[test]
    fn add_role_requires_manage_roles() {
        let op = SpacePatchOp::AddRole(role_fixture());
        assert_eq!(required_permissions_for_op(&op), &[MANAGE_ROLES]);
    }

    #[test]
    fn remove_role_requires_manage_roles() {
        let op = SpacePatchOp::RemoveRole(Id::from("r1"));
        assert_eq!(required_permissions_for_op(&op), &[MANAGE_ROLES]);
    }

    #[test]
    fn update_role_requires_manage_roles() {
        let mut patch = RolePatch::default();
        patch.name = Some("Renamed".to_owned());
        let op = SpacePatchOp::UpdateRole {
            id: Id::from("r1"),
            patch,
        };
        assert_eq!(required_permissions_for_op(&op), &[MANAGE_ROLES]);
    }

    #[test]
    fn add_member_requires_manage_members_only() {
        let op = SpacePatchOp::AddMember {
            user_id: Id::from("u1"),
            role_ids: vec![Id::from("r1"), Id::from("r2")],
        };
        // Spec table: `addMembers` is `manage_members`. The role_ids field on
        // AddMember does NOT additionally require `manage_roles` — only
        // `updateMembers` with a roleIds change does.
        assert_eq!(required_permissions_for_op(&op), &[MANAGE_MEMBERS]);
    }

    #[test]
    fn remove_member_requires_manage_members() {
        let op = SpacePatchOp::RemoveMember(Id::from("u1"));
        assert_eq!(required_permissions_for_op(&op), &[MANAGE_MEMBERS]);
    }

    #[test]
    fn update_member_nick_only_requires_manage_members_only() {
        let mut patch = MemberPatch::default();
        patch.nick = Some(Clearable::Set("Captain".to_owned()));
        let op = SpacePatchOp::UpdateMember {
            user_id: Id::from("u1"),
            patch,
        };
        assert_eq!(required_permissions_for_op(&op), &[MANAGE_MEMBERS]);
    }

    #[test]
    fn update_member_role_ids_change_requires_both() {
        let mut patch = MemberPatch::default();
        patch.role_ids = Some(vec![Id::from("r1")]);
        let op = SpacePatchOp::UpdateMember {
            user_id: Id::from("u1"),
            patch,
        };
        assert_eq!(
            required_permissions_for_op(&op),
            &[MANAGE_MEMBERS, MANAGE_ROLES]
        );
    }

    #[test]
    fn update_member_role_ids_empty_still_counts_as_change() {
        // An empty Vec is still `Some(_)` and represents an explicit
        // "set roleIds to []" mutation, which requires manage_roles.
        let mut patch = MemberPatch::default();
        patch.role_ids = Some(vec![]);
        let op = SpacePatchOp::UpdateMember {
            user_id: Id::from("u1"),
            patch,
        };
        assert_eq!(
            required_permissions_for_op(&op),
            &[MANAGE_MEMBERS, MANAGE_ROLES]
        );
    }

    #[test]
    fn add_channel_requires_manage_channels() {
        let op = SpacePatchOp::AddChannel(channel_create_fixture());
        assert_eq!(required_permissions_for_op(&op), &[MANAGE_CHANNELS]);
    }

    #[test]
    fn remove_channel_requires_manage_channels() {
        let op = SpacePatchOp::RemoveChannel(Id::from("ch1"));
        assert_eq!(required_permissions_for_op(&op), &[MANAGE_CHANNELS]);
    }

    #[test]
    fn update_channel_requires_manage_channels() {
        let mut patch = ChannelPatch::default();
        patch.name = Some("renamed".to_owned());
        let op = SpacePatchOp::UpdateChannel {
            id: Id::from("ch1"),
            patch,
        };
        assert_eq!(required_permissions_for_op(&op), &[MANAGE_CHANNELS]);
    }

    #[test]
    fn add_category_requires_manage_channels() {
        let op = SpacePatchOp::AddCategory(category_fixture());
        assert_eq!(required_permissions_for_op(&op), &[MANAGE_CHANNELS]);
    }

    #[test]
    fn remove_category_requires_manage_channels() {
        let op = SpacePatchOp::RemoveCategory(Id::from("cat1"));
        assert_eq!(required_permissions_for_op(&op), &[MANAGE_CHANNELS]);
    }

    #[test]
    fn update_category_requires_manage_channels() {
        let mut patch = CategoryPatch::default();
        patch.name = Some("Voice".to_owned());
        let op = SpacePatchOp::UpdateCategory {
            id: Id::from("cat1"),
            patch,
        };
        assert_eq!(required_permissions_for_op(&op), &[MANAGE_CHANNELS]);
    }

    // The constants are also the verbatim permission strings used on the
    // wire by spec-conforming backends and clients. Pin the literal bytes
    // so a future rename here is caught immediately.
    #[test]
    fn permission_constants_have_spec_literal_values() {
        assert_eq!(MANAGE_SPACE, "manage_space");
        assert_eq!(MANAGE_ROLES, "manage_roles");
        assert_eq!(MANAGE_MEMBERS, "manage_members");
        assert_eq!(MANAGE_CHANNELS, "manage_channels");
    }

    // Helper returns `&'static [&'static str]`: no allocation. Pin that the
    // function pointer signature is stable by calling and binding to that
    // exact type.
    #[test]
    fn helper_returns_static_slice() {
        fn assert_static_slice<F>(_f: F)
        where
            F: Fn(&SpacePatchOp) -> &'static [&'static str],
        {
        }
        assert_static_slice(required_permissions_for_op);
    }
}
