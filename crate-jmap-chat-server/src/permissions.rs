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

/// Result of [`required_permissions_for_op`] — typed alternative to
/// a sentinel-string return value.
///
/// `SpacePatchOp` is `#[non_exhaustive]` upstream, so a future-added
/// variant cannot be matched at compile time from this downstream
/// crate. The [`UnknownOp`](Self::UnknownOp) arm signals that
/// situation explicitly; backends MUST reject ops that produce
/// `UnknownOp` rather than treating an empty permission set as
/// "no permissions required".
///
/// The enum is `#[non_exhaustive]` so future variants (e.g. a
/// `Conditional` arm carrying a runtime predicate) can be added
/// without a SemVer break. Match arms in consumer code should
/// include a `_ => { /* fail closed */ }` catch-all.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequiredPermissions {
    /// The op's permission requirement is known and enumerated by
    /// the draft. The contained slice is the list of permission
    /// strings the caller MUST hold to apply this op. A caller's
    /// effective-permission set must be a superset of this slice.
    Known(&'static [&'static str]),
    /// The op variant is not recognized by this version of
    /// `required_permissions_for_op`. This happens when a
    /// downstream-extended [`SpacePatchOp`] adds a new variant
    /// that the kit predates. Backends MUST reject the op rather
    /// than authorize it as zero-permissions-required.
    UnknownOp,
}

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
/// Return shape: [`RequiredPermissions::Known`] carries the spec-
/// derived permission slice; [`RequiredPermissions::UnknownOp`]
/// signals that the op variant is not recognized and the backend
/// MUST reject the patch (fail closed). The typed return value
/// replaced an earlier `&'static [&'static str]` shape that used a
/// magic sentinel string `"__unknown_space_patch_op__"` to signal
/// the unknown case; that contract was load-bearing on a name no
/// public API exposed.
///
/// This helper is pure and has no side effects. Backends consume it
/// inside [`ChatBackend::apply_space_patch`] after resolving the
/// caller's effective permissions. Handlers MUST NOT consume this
/// helper for permission gating — gates are backend-canonical per
/// workspace AGENTS.md.
///
/// [`PatchObject`]: jmap_types::PatchObject
/// [`ChatBackend::apply_space_patch`]: crate::backend::ChatBackend::apply_space_patch
pub fn required_permissions_for_op(op: &SpacePatchOp) -> RequiredPermissions {
    match op {
        // Role family — draft §Space/set lines 1102, 1105, 1108.
        SpacePatchOp::AddRole(_)
        | SpacePatchOp::RemoveRole(_)
        | SpacePatchOp::UpdateRole { .. } => RequiredPermissions::Known(&[MANAGE_ROLES]),

        // Member add/remove — draft §Space/set lines 1111, 1114.
        SpacePatchOp::AddMember(_) | SpacePatchOp::RemoveMember(_) => {
            RequiredPermissions::Known(&[MANAGE_MEMBERS])
        }

        // Member update — draft §Space/set line 1117. Modifying `roleIds`
        // requires `manage_roles` in addition to `manage_members`; nick-only
        // edits require only `manage_members`.
        SpacePatchOp::UpdateMember { patch, .. } => {
            if patch.role_ids.is_some() {
                RequiredPermissions::Known(&[MANAGE_MEMBERS, MANAGE_ROLES])
            } else {
                RequiredPermissions::Known(&[MANAGE_MEMBERS])
            }
        }

        // Channel family — draft §Space/set lines 1120, 1123, 1126.
        SpacePatchOp::AddChannel(_)
        | SpacePatchOp::RemoveChannel(_)
        | SpacePatchOp::UpdateChannel { .. } => RequiredPermissions::Known(&[MANAGE_CHANNELS]),

        // Category family — draft §Space/set line 1129. Categories are a
        // sub-property of the channel-set per the draft; the same permission
        // gates both.
        SpacePatchOp::AddCategory(_)
        | SpacePatchOp::RemoveCategory(_)
        | SpacePatchOp::UpdateCategory { .. } => RequiredPermissions::Known(&[MANAGE_CHANNELS]),

        // `SpacePatchOp` is `#[non_exhaustive]` upstream. A future-added
        // variant cannot be matched at compile time from this downstream
        // crate. Fail closed via the typed `UnknownOp` arm; backends MUST
        // reject ops that produce this value.
        _ => RequiredPermissions::UnknownOp,
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
        assert_eq!(
            required_permissions_for_op(&op),
            RequiredPermissions::Known(&[MANAGE_ROLES])
        );
    }

    #[test]
    fn remove_role_requires_manage_roles() {
        let op = SpacePatchOp::RemoveRole(Id::from("r1"));
        assert_eq!(
            required_permissions_for_op(&op),
            RequiredPermissions::Known(&[MANAGE_ROLES])
        );
    }

    #[test]
    fn update_role_requires_manage_roles() {
        let mut patch = RolePatch::default();
        patch.name = Some("Renamed".to_owned());
        let op = SpacePatchOp::UpdateRole {
            id: Id::from("r1"),
            patch,
        };
        assert_eq!(
            required_permissions_for_op(&op),
            RequiredPermissions::Known(&[MANAGE_ROLES])
        );
    }

    #[test]
    fn add_member_requires_manage_members_only() {
        let op = SpacePatchOp::AddMember(jmap_chat_types::MemberCreate::new(
            Id::from("u1"),
            vec![Id::from("r1"), Id::from("r2")],
        ));
        // Spec table: `addMembers` is `manage_members`. The role_ids field on
        // AddMember does NOT additionally require `manage_roles` — only
        // `updateMembers` with a roleIds change does.
        assert_eq!(
            required_permissions_for_op(&op),
            RequiredPermissions::Known(&[MANAGE_MEMBERS])
        );
    }

    #[test]
    fn remove_member_requires_manage_members() {
        let op = SpacePatchOp::RemoveMember(Id::from("u1"));
        assert_eq!(
            required_permissions_for_op(&op),
            RequiredPermissions::Known(&[MANAGE_MEMBERS])
        );
    }

    #[test]
    fn update_member_nick_only_requires_manage_members_only() {
        let mut patch = MemberPatch::default();
        patch.nick = Some(Clearable::Set("Captain".to_owned()));
        let op = SpacePatchOp::UpdateMember {
            user_id: Id::from("u1"),
            patch,
        };
        assert_eq!(
            required_permissions_for_op(&op),
            RequiredPermissions::Known(&[MANAGE_MEMBERS])
        );
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
            RequiredPermissions::Known(&[MANAGE_MEMBERS, MANAGE_ROLES])
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
            RequiredPermissions::Known(&[MANAGE_MEMBERS, MANAGE_ROLES])
        );
    }

    #[test]
    fn add_channel_requires_manage_channels() {
        let op = SpacePatchOp::AddChannel(channel_create_fixture());
        assert_eq!(
            required_permissions_for_op(&op),
            RequiredPermissions::Known(&[MANAGE_CHANNELS])
        );
    }

    #[test]
    fn remove_channel_requires_manage_channels() {
        let op = SpacePatchOp::RemoveChannel(Id::from("ch1"));
        assert_eq!(
            required_permissions_for_op(&op),
            RequiredPermissions::Known(&[MANAGE_CHANNELS])
        );
    }

    #[test]
    fn update_channel_requires_manage_channels() {
        let mut patch = ChannelPatch::default();
        patch.name = Some("renamed".to_owned());
        let op = SpacePatchOp::UpdateChannel {
            id: Id::from("ch1"),
            patch,
        };
        assert_eq!(
            required_permissions_for_op(&op),
            RequiredPermissions::Known(&[MANAGE_CHANNELS])
        );
    }

    #[test]
    fn add_category_requires_manage_channels() {
        let op = SpacePatchOp::AddCategory(category_fixture());
        assert_eq!(
            required_permissions_for_op(&op),
            RequiredPermissions::Known(&[MANAGE_CHANNELS])
        );
    }

    #[test]
    fn remove_category_requires_manage_channels() {
        let op = SpacePatchOp::RemoveCategory(Id::from("cat1"));
        assert_eq!(
            required_permissions_for_op(&op),
            RequiredPermissions::Known(&[MANAGE_CHANNELS])
        );
    }

    #[test]
    fn update_category_requires_manage_channels() {
        let mut patch = CategoryPatch::default();
        patch.name = Some("Voice".to_owned());
        let op = SpacePatchOp::UpdateCategory {
            id: Id::from("cat1"),
            patch,
        };
        assert_eq!(
            required_permissions_for_op(&op),
            RequiredPermissions::Known(&[MANAGE_CHANNELS])
        );
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

    // Helper returns `RequiredPermissions` carrying `&'static [&'static str]`
    // for the Known arm: no allocation. Pin the function pointer signature so
    // a future return-type drift is caught at compile time.
    #[test]
    fn helper_returns_typed_known_or_unknown() {
        fn assert_typed_return<F>(_f: F)
        where
            F: Fn(&SpacePatchOp) -> RequiredPermissions,
        {
        }
        assert_typed_return(required_permissions_for_op);
    }

    // Round-trip the typed return shape: a Known arm must carry a non-empty,
    // static slice; the UnknownOp arm must be constructible as a literal so
    // consumers can write match arms that fail closed.
    #[test]
    fn unknown_op_variant_is_distinct_from_any_known_arm() {
        // Two Known values with different content are unequal; a Known value
        // is never equal to UnknownOp.
        let a = RequiredPermissions::Known(&[MANAGE_ROLES]);
        let b = RequiredPermissions::Known(&[MANAGE_MEMBERS]);
        let u = RequiredPermissions::UnknownOp;
        assert_ne!(a, b);
        assert_ne!(a, u);
        assert_ne!(b, u);
        assert_eq!(u, RequiredPermissions::UnknownOp);
    }
}
