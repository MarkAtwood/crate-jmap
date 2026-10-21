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

use std::collections::HashSet;

use crate::backend::SpacePatchOp;
use jmap_chat_types::ChannelPermission;

/// Permission string: top-level Space metadata mutations.
///
/// Per draft-atwood-jmap-chat-00 §Space/set L1256-1257 (metadata patch
/// keys `name`, `description`, `iconBlobId`) and §Space L812, L815
/// (`isPublic` and `isPubliclyPreviewable`: "Mutable by members with
/// `"manage_space"` permission"). §Space/set names no patch key for
/// those two (bd:JMAP-y3f2.13).
pub const MANAGE_SPACE: &str = "manage_space";

/// Permission string: role-set mutations.
///
/// Per draft-atwood-jmap-chat-00 §Space/set L1260, L1263, L1266
/// (`addRoles`, `removeRoles`, `updateRoles`).
pub const MANAGE_ROLES: &str = "manage_roles";

/// Permission string: member-set mutations.
///
/// Per draft-atwood-jmap-chat-00 §Space/set L1269, L1272, L1277
/// (`addMembers`, `removeMembers`, `updateMembers`).
pub const MANAGE_MEMBERS: &str = "manage_members";

/// Permission string: channel- and category-set mutations.
///
/// Per draft-atwood-jmap-chat-00 §Space/set L1280-1295
/// (`addChannels`, `removeChannels`, `updateChannels`, `addCategories`,
/// `removeCategories`, `updateCategories`).
pub const MANAGE_CHANNELS: &str = "manage_channels";

/// Permission string: modify a channel Chat's `pinnedMessageIds`.
///
/// Per draft-atwood-jmap-chat-00 §Chat `pinnedMessageIds`: "For channel
/// chats, only members with the `"pin"` Space permission may modify this
/// list."
pub const PIN: &str = "pin";

/// Permission string: create, update or destroy a SpaceBan.
///
/// Per draft-atwood-jmap-chat-00 §SpaceBan/set: create, update and
/// destroy each "Requires `"ban"` permission in the Space."
pub const BAN: &str = "ban";

/// Permission string: send or keep a Message with a non-empty
/// `broadcastMentions` array in a channel Chat.
///
/// Per draft-atwood-jmap-chat-00 §BroadcastMention: "Sending a Message
/// with a non-empty `broadcastMentions` array requires the
/// `"mention_broadcast"` Space permission."
pub const MENTION_BROADCAST: &str = "mention_broadcast";

/// Permission string: see a channel and its messages.
///
/// Per draft-atwood-jmap-chat-00 §Message/query: "For channel Chats, the
/// server MUST verify the caller holds the `"view"` permission before
/// returning messages in `Message/get` or `Message/query` responses".
pub const VIEW: &str = "view";

/// Permission string: send a Message in a channel Chat.
///
/// Per draft-atwood-jmap-chat-00 §Message/set: "For channel Chats, the
/// server MUST verify the sender holds the `"send"` permission (resolved
/// per {{space-permissions}}) before accepting the message".
pub const SEND: &str = "send";

/// Resolve a member's effective permissions in one channel Chat, per
/// draft-atwood-jmap-chat-00 §Space Permission Resolution.
///
/// `space_permissions` is step 1, the union of `permissions` across every
/// SpaceRole the member holds, including the implicit `@everyone` role.
/// `role_ids` are the SpaceRole ids the member holds, `member_id` is its
/// SpaceMember `id`, and `overrides` is the channel's
/// `permissionOverrides`. Steps 2 to 5 then apply, in order: `deny` of
/// role-targeted records matching `role_ids`, their `allow`, `deny` of
/// the record targeting `member_id`, and its `allow`.
///
/// Within one step every entry has the same effect (remove or add), so
/// the draft's "ascending position order" for role-targeted records
/// cannot change the result and is not consulted.
///
/// Records whose `targetType` is neither `"role"` nor `"member"` match
/// nothing. The draft gives the implicit `@everyone` role no id
/// (§SpaceRole: it "is not included in the `roles` array"), so no
/// role-targeted record can address it; restrict or grant the floor with
/// member-targeted records instead.
pub fn resolve_channel_permissions(
    space_permissions: HashSet<String>,
    member_id: &str,
    role_ids: &HashSet<&str>,
    overrides: &[ChannelPermission],
) -> HashSet<String> {
    let role_records: Vec<&ChannelPermission> = overrides
        .iter()
        .filter(|o| o.target_type.as_str() == "role" && role_ids.contains(o.target_id.as_ref()))
        .collect();
    let member_records: Vec<&ChannelPermission> = overrides
        .iter()
        .filter(|o| o.target_type.as_str() == "member" && o.target_id.as_ref() == member_id)
        .collect();
    let mut perms = space_permissions;
    for records in [role_records, member_records] {
        for o in &records {
            for p in &o.deny {
                perms.remove(p.as_str());
            }
        }
        for o in &records {
            perms.extend(o.allow.iter().map(|p| p.as_str().to_owned()));
        }
    }
    perms
}

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
/// L1256-1295 (jmap-chat-spec 91672c9).
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
/// - `manage_roles` — `addRoles`, `removeRoles`, `updateRoles`, and
///   `updateMembers` entries that modify `roleIds`.
/// - `manage_members` — `addMembers`, `removeMembers`, and
///   `updateMembers` entries that change `nick`.
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
        // Role family — draft §Space/set L1260, L1263, L1266.
        SpacePatchOp::AddRole(_)
        | SpacePatchOp::RemoveRole(_)
        | SpacePatchOp::UpdateRole { .. } => RequiredPermissions::Known(&[MANAGE_ROLES]),

        // Member add/remove — draft §Space/set L1269, L1272.
        SpacePatchOp::AddMember(_) | SpacePatchOp::RemoveMember(_) => {
            RequiredPermissions::Known(&[MANAGE_MEMBERS])
        }

        // Member update — draft-atwood-jmap-chat-00 L1277: "Role changes
        // require `"manage_roles"`. Changing another member's `nick`
        // requires `"manage_members"`." A `roleIds` change alone needs only
        // `manage_roles`; the role-position hierarchy check in the backend
        // bounds which roles it may grant or remove. The caller's own
        // nick-only edit is exempt, but this helper has no caller identity,
        // so the backend gate applies that exemption.
        SpacePatchOp::UpdateMember { patch, .. } => match (&patch.role_ids, &patch.nick) {
            (Some(_), Some(_)) => RequiredPermissions::Known(&[MANAGE_MEMBERS, MANAGE_ROLES]),
            (Some(_), None) => RequiredPermissions::Known(&[MANAGE_ROLES]),
            (None, _) => RequiredPermissions::Known(&[MANAGE_MEMBERS]),
        },

        // Channel family — draft §Space/set L1280, L1283, L1286.
        SpacePatchOp::AddChannel(_)
        | SpacePatchOp::RemoveChannel(_)
        | SpacePatchOp::UpdateChannel { .. } => RequiredPermissions::Known(&[MANAGE_CHANNELS]),

        // Category family — draft §Space/set L1289, L1292, L1295. Categories are a
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
    // §Space/set L1256-1295 (jmap-chat-spec 91672c9). Each test constructs the SpacePatchOp variant from a
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

    // Draft §Space/set `updateMembers`: "Changing another member's `nick`
    // requires `"manage_members"`. A member MAY set their own `nick` by
    // submitting `updateMembers` with a single entry whose `id` is their own
    // and which contains only `nick`; the server MUST permit this regardless
    // of the caller's permission set." This helper sees no caller, so it
    // states the other-member requirement; the self-nick exemption lives in
    // the backend gate (tests/space_update_members.rs).
    #[test]
    fn update_other_member_nick_only_requires_manage_members_only() {
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

    // Draft L1277: "Role changes require `"manage_roles"`." No other
    // permission is named for a `roleIds`-only entry.
    #[test]
    fn update_member_role_ids_change_requires_manage_roles_only() {
        let mut patch = MemberPatch::default();
        patch.role_ids = Some(vec![Id::from("r1")]);
        let op = SpacePatchOp::UpdateMember {
            user_id: Id::from("u1"),
            patch,
        };
        assert_eq!(
            required_permissions_for_op(&op),
            RequiredPermissions::Known(&[MANAGE_ROLES])
        );
    }

    // Draft L1277: "Role changes require `"manage_roles"`. Changing another
    // member's `nick` requires `"manage_members"`." An entry doing both needs
    // both.
    #[test]
    fn update_member_role_ids_and_nick_requires_both() {
        let mut patch = MemberPatch::default();
        patch.role_ids = Some(vec![Id::from("r1")]);
        patch.nick = Some(Clearable::Set("Captain".to_owned()));
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
            RequiredPermissions::Known(&[MANAGE_ROLES])
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

    // Oracle for `resolve_channel_permissions`: draft-atwood-jmap-chat-00
    // §Space Permission Resolution (jmap-chat-spec 91672c9, L1619-1631),
    // quoted per test. Overrides are hand-written JSON shaped after
    // §ChannelPermission: "`targetId` (String): A SpaceRole id or a
    // SpaceMember `id`. `targetType` (String): `"role"` or `"member"`."

    use serde_json::json;

    fn overrides(v: serde_json::Value) -> Vec<ChannelPermission> {
        serde_json::from_value(v).expect("hand-written ChannelPermission[]")
    }

    fn set(items: &[&str]) -> HashSet<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    fn resolve(base: &[&str], roles: &[&str], o: serde_json::Value) -> HashSet<String> {
        let role_ids: HashSet<&str> = roles.iter().copied().collect();
        resolve_channel_permissions(set(base), "m1", &role_ids, &overrides(o))
    }

    // Step 1: "Compute the union of `permissions` across all SpaceRoles
    // held by the member, including the implicit `@everyone` role." With
    // no overrides the Space-level union is the result.
    #[test]
    fn step1_no_overrides_keeps_space_union() {
        assert_eq!(
            resolve(&["view", "send"], &["r1"], json!([])),
            set(&["view", "send"])
        );
    }

    // Step 2: "Apply `deny` entries from any ChannelPermission records
    // matching the member's roles (`targetType: "role"`)". A record for a
    // role the member does not hold does not apply.
    #[test]
    fn step2_role_deny_removes_permission() {
        let o = json!([
            { "targetId": "r1", "targetType": "role", "allow": [], "deny": ["send"] },
            { "targetId": "r9", "targetType": "role", "allow": [], "deny": ["view"] }
        ]);
        assert_eq!(resolve(&["view", "send"], &["r1"], o), set(&["view"]));
    }

    // Step 3: "Apply `allow` entries from the same role-targeted records"
    // — after step 2, so one role's allow beats another role's deny.
    #[test]
    fn step3_role_allow_overrides_role_deny() {
        let o = json!([
            { "targetId": "r1", "targetType": "role", "allow": [], "deny": ["send"] },
            { "targetId": "r2", "targetType": "role", "allow": ["send"], "deny": [] }
        ]);
        assert_eq!(
            resolve(&["view", "send"], &["r1", "r2"], o),
            set(&["view", "send"])
        );
    }

    // Step 4: "Apply `deny` entries from any ChannelPermission record
    // matching the member directly (`targetType: "member"`)" — after step
    // 3, so a member deny beats a role allow.
    #[test]
    fn step4_member_deny_overrides_role_allow() {
        let o = json!([
            { "targetId": "r1", "targetType": "role", "allow": ["send"], "deny": [] },
            { "targetId": "m1", "targetType": "member", "allow": [], "deny": ["send"] }
        ]);
        assert_eq!(resolve(&["view"], &["r1"], o), set(&["view"]));
    }

    // Step 5: "Apply `allow` entries from the same member-targeted
    // record." — last, so a member allow beats a role deny of the same
    // permission, and a record for another member does not apply.
    #[test]
    fn step5_member_allow_overrides_role_deny() {
        let o = json!([
            { "targetId": "r1", "targetType": "role", "allow": [], "deny": ["send", "view"] },
            { "targetId": "m1", "targetType": "member", "allow": ["send"], "deny": [] },
            { "targetId": "m2", "targetType": "member", "allow": ["view"], "deny": [] }
        ]);
        assert_eq!(resolve(&["view", "send"], &["r1"], o), set(&["send"]));
    }

    // A role-targeted record never matches a member id and vice versa:
    // §ChannelPermission `targetType` distinguishes "a SpaceRole id or a
    // SpaceMember `id`".
    #[test]
    fn target_type_selects_the_id_space() {
        let o = json!([
            { "targetId": "m1", "targetType": "role", "allow": ["pin"], "deny": [] },
            { "targetId": "r1", "targetType": "member", "allow": ["ban"], "deny": [] }
        ]);
        assert_eq!(resolve(&["view"], &["r1"], o), set(&["view"]));
    }
}
