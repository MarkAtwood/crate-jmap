//! Pure permission-vocabulary helpers for backend authorization gating.
//!
//! This module hosts pure (no I/O, no state) mappings from server-side
//! mutation values onto the [`ChatPermission`] vocabulary defined by
//! draft-atwood-jmap-chat-00 §SpaceRole `permissions` and gated per
//! §Space/set. Backend implementations of
//! [`ChatBackend::apply_space_patch`] consume these helpers to gate
//! per-op authorization after resolving the caller's effective
//! permission set in a Space.
//!
//! Per workspace AGENTS.md "backend canonical" policy, handlers MUST NOT
//! call these helpers for permission gating — only backends do.
//!
//! [`ChatBackend::apply_space_patch`]: crate::ChatBackend::apply_space_patch

use std::collections::HashSet;

use crate::SpacePatchOp;
use jmap_chat_types::{ChannelPermission, ChatPermission};

/// Resolve a member's effective permissions in one channel Chat, per
/// draft-atwood-jmap-chat-00 §Space Permission Resolution.
///
/// `space_permissions` is step 1, the union of `permissions` across every
/// SpaceRole the member holds, including the implicit `@everyone` role.
/// `space_id` is the id of the Space holding the channel, `role_ids` are
/// the SpaceRole ids the member holds, `member_id` is its SpaceMember
/// `id`, and `overrides` is the channel's `permissionOverrides`. Steps 2
/// to 5 then apply, in order: `deny` of role-targeted records matching
/// `role_ids` or `space_id`, their `allow`, `deny` of the record
/// targeting `member_id`, and its `allow`.
///
/// Within one step every entry has the same effect (remove or add), so
/// the draft's "ascending position order" for role-targeted records
/// cannot change the result and is not consulted.
///
/// Records whose `targetType` is neither `"role"` nor `"member"` match
/// nothing. Every member holds the implicit `@everyone` role, which has
/// no SpaceRole id; a role-targeted record whose `targetId` is
/// `space_id` addresses it (§ChannelPermission `targetId`: "A record
/// with `targetType: "role"` whose `targetId` is the id of the Space
/// itself addresses the implicit `@everyone` role"), so it matches every
/// member.
///
/// A `deny` entry removes every permission with the same wire string
/// ([`ChatPermission::as_str`]), so a forged
/// [`ChatPermission::Other`] spelling of a known name still denies it;
/// an `allow` entry adds the permission as given, so such a spelling
/// never grants the known variant.
pub fn resolve_channel_permissions(
    space_permissions: HashSet<ChatPermission>,
    space_id: &str,
    member_id: &str,
    role_ids: &HashSet<&str>,
    overrides: &[ChannelPermission],
) -> HashSet<ChatPermission> {
    let role_records: Vec<&ChannelPermission> = overrides
        .iter()
        .filter(|o| {
            let target = o.target_id.as_ref();
            o.target_type.as_str() == "role" && (target == space_id || role_ids.contains(target))
        })
        .collect();
    let member_records: Vec<&ChannelPermission> = overrides
        .iter()
        .filter(|o| o.target_type.as_str() == "member" && o.target_id.as_ref() == member_id)
        .collect();
    let mut perms = space_permissions;
    for records in [role_records, member_records] {
        for o in &records {
            for p in &o.deny {
                perms.retain(|q| q.as_str() != p.as_str());
            }
        }
        for o in &records {
            perms.extend(o.allow.iter().cloned());
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
    /// the draft. The contained slice is the list of permissions
    /// the caller MUST hold to apply this op. A caller's
    /// effective-permission set must be a superset of this slice.
    Known(&'static [ChatPermission]),
    /// The op variant is not recognized by this version of
    /// `required_permissions_for_op`. This happens when a
    /// downstream-extended [`SpacePatchOp`] adds a new variant
    /// that the kit predates. Backends MUST reject the op rather
    /// than authorize it as zero-permissions-required.
    UnknownOp,
}

/// Returns the set of permissions required to apply this
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
/// replaced an earlier slice-only shape that used a magic sentinel
/// string `"__unknown_space_patch_op__"` to signal the unknown case;
/// that contract was load-bearing on a name no public API exposed.
///
/// This helper is pure and has no side effects. Backends consume it
/// inside [`ChatBackend::apply_space_patch`] after resolving the
/// caller's effective permissions. Handlers MUST NOT consume this
/// helper for permission gating — gates are backend-canonical per
/// workspace AGENTS.md.
///
/// [`PatchObject`]: jmap_types::PatchObject
/// [`ChatBackend::apply_space_patch`]: crate::ChatBackend::apply_space_patch
pub fn required_permissions_for_op(op: &SpacePatchOp) -> RequiredPermissions {
    match op {
        // Role family — draft §Space/set L1260, L1263, L1266.
        SpacePatchOp::AddRole(_)
        | SpacePatchOp::RemoveRole(_)
        | SpacePatchOp::UpdateRole { .. } => {
            RequiredPermissions::Known(&[ChatPermission::ManageRoles])
        }

        // Member add/remove — draft §Space/set L1269, L1272.
        SpacePatchOp::AddMember(_) | SpacePatchOp::RemoveMember(_) => {
            RequiredPermissions::Known(&[ChatPermission::ManageMembers])
        }

        // Member update — draft-atwood-jmap-chat-00 L1277: "Role changes
        // require `"manage_roles"`. Changing another member's `nick`
        // requires `"manage_members"`." A `roleIds` change alone needs only
        // `manage_roles`; the role-position hierarchy check in the backend
        // bounds which roles it may grant or remove. The caller's own
        // nick-only edit is exempt, but this helper has no caller identity,
        // so the backend gate applies that exemption.
        SpacePatchOp::UpdateMember { patch, .. } => match (&patch.role_ids, &patch.nick) {
            (Some(_), Some(_)) => RequiredPermissions::Known(&[
                ChatPermission::ManageMembers,
                ChatPermission::ManageRoles,
            ]),
            (Some(_), None) => RequiredPermissions::Known(&[ChatPermission::ManageRoles]),
            (None, _) => RequiredPermissions::Known(&[ChatPermission::ManageMembers]),
        },

        // Channel family — draft §Space/set L1280, L1283, L1286.
        SpacePatchOp::AddChannel(_)
        | SpacePatchOp::RemoveChannel(_)
        | SpacePatchOp::UpdateChannel { .. } => {
            RequiredPermissions::Known(&[ChatPermission::ManageChannels])
        }

        // Category family — draft §Space/set L1289, L1292, L1295. Categories are a
        // sub-property of the channel-set per the draft; the same permission
        // gates both.
        SpacePatchOp::AddCategory(_)
        | SpacePatchOp::RemoveCategory(_)
        | SpacePatchOp::UpdateCategory { .. } => {
            RequiredPermissions::Known(&[ChatPermission::ManageChannels])
        }

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
    use jmap_chat_types::Clearable;
    use jmap_chat_types::{
        CategoryCreate, CategoryPatch, ChannelCreate, ChannelPatch, MemberPatch, RoleCreate,
        RolePatch,
    };
    use jmap_types::Id;

    // Oracle for the spec-line → permission mapping: draft-atwood-jmap-chat-00
    // §Space/set L1256-1295 (jmap-chat-spec 91672c9). Each test constructs the SpacePatchOp variant from a
    // hand-built payload (deserialized from spec-shaped JSON for types without
    // `Default`, or built via `Default::default()` for the patch types) and
    // asserts the slice returned by the helper matches the table in the
    // rustdoc.
    //
    // `RoleCreate`, `CategoryCreate`, and `ChannelCreate` are `#[non_exhaustive]`
    // without `Default`, so they are constructed via `serde_json::from_value`
    // from hand-written JSON shaped after draft-atwood-jmap-chat-00 §4.12
    // and §Space/set. This satisfies the workspace
    // independent-oracle rule: the JSON is hand-typed from the spec, not
    // derived from the code under test.

    fn role_fixture() -> RoleCreate {
        serde_json::from_value(serde_json::json!({
            "name": "Moderator",
            "permissions": ["chat:read"],
            "position": 0
        }))
        .expect("role fixture must deserialize")
    }

    fn category_fixture() -> CategoryCreate {
        serde_json::from_value(serde_json::json!({
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
            RequiredPermissions::Known(&[ChatPermission::ManageRoles])
        );
    }

    #[test]
    fn remove_role_requires_manage_roles() {
        let op = SpacePatchOp::RemoveRole(Id::from("r1"));
        assert_eq!(
            required_permissions_for_op(&op),
            RequiredPermissions::Known(&[ChatPermission::ManageRoles])
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
            RequiredPermissions::Known(&[ChatPermission::ManageRoles])
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
            RequiredPermissions::Known(&[ChatPermission::ManageMembers])
        );
    }

    #[test]
    fn remove_member_requires_manage_members() {
        let op = SpacePatchOp::RemoveMember(Id::from("u1"));
        assert_eq!(
            required_permissions_for_op(&op),
            RequiredPermissions::Known(&[ChatPermission::ManageMembers])
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
            RequiredPermissions::Known(&[ChatPermission::ManageMembers])
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
            RequiredPermissions::Known(&[ChatPermission::ManageRoles])
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
            RequiredPermissions::Known(&[
                ChatPermission::ManageMembers,
                ChatPermission::ManageRoles
            ])
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
            RequiredPermissions::Known(&[ChatPermission::ManageRoles])
        );
    }

    #[test]
    fn add_channel_requires_manage_channels() {
        let op = SpacePatchOp::AddChannel(channel_create_fixture());
        assert_eq!(
            required_permissions_for_op(&op),
            RequiredPermissions::Known(&[ChatPermission::ManageChannels])
        );
    }

    #[test]
    fn remove_channel_requires_manage_channels() {
        let op = SpacePatchOp::RemoveChannel(Id::from("ch1"));
        assert_eq!(
            required_permissions_for_op(&op),
            RequiredPermissions::Known(&[ChatPermission::ManageChannels])
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
            RequiredPermissions::Known(&[ChatPermission::ManageChannels])
        );
    }

    #[test]
    fn add_category_requires_manage_channels() {
        let op = SpacePatchOp::AddCategory(category_fixture());
        assert_eq!(
            required_permissions_for_op(&op),
            RequiredPermissions::Known(&[ChatPermission::ManageChannels])
        );
    }

    #[test]
    fn remove_category_requires_manage_channels() {
        let op = SpacePatchOp::RemoveCategory(Id::from("cat1"));
        assert_eq!(
            required_permissions_for_op(&op),
            RequiredPermissions::Known(&[ChatPermission::ManageChannels])
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
            RequiredPermissions::Known(&[ChatPermission::ManageChannels])
        );
    }

    // The required permissions serialize to the verbatim names of
    // draft-atwood-jmap-chat-00 §SpaceRole `permissions` (jmap-chat-spec
    // 33702a5, L798-807): `"manage_channels"`, `"manage_members"`,
    // `"manage_roles"`. Pin the wire bytes so a rename is caught.
    #[test]
    fn required_permissions_serialize_to_spec_names() {
        let mut both = MemberPatch::default();
        both.role_ids = Some(vec![Id::from("r1")]);
        both.nick = Some(Clearable::Set("Captain".to_owned()));
        let cases = [
            (
                SpacePatchOp::UpdateMember {
                    user_id: Id::from("u1"),
                    patch: both,
                },
                serde_json::json!(["manage_members", "manage_roles"]),
            ),
            (
                SpacePatchOp::RemoveChannel(Id::from("ch1")),
                serde_json::json!(["manage_channels"]),
            ),
        ];
        for (op, wire) in cases {
            let RequiredPermissions::Known(slice) = required_permissions_for_op(&op) else {
                panic!("op must have a known requirement");
            };
            assert_eq!(serde_json::to_value(slice).expect("serialize"), wire);
        }
    }

    // Helper returns `RequiredPermissions` carrying `&'static [ChatPermission]`
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
        let a = RequiredPermissions::Known(&[ChatPermission::ManageRoles]);
        let b = RequiredPermissions::Known(&[ChatPermission::ManageMembers]);
        let u = RequiredPermissions::UnknownOp;
        assert_ne!(a, b);
        assert_ne!(a, u);
        assert_ne!(b, u);
        assert_eq!(u, RequiredPermissions::UnknownOp);
    }

    // Oracle for `resolve_channel_permissions`: draft-atwood-jmap-chat-00
    // §Space Permission Resolution (jmap-chat-spec 33702a5, L1700-1709),
    // quoted per test. Overrides are hand-written JSON shaped after
    // §ChannelPermission: "`targetId` (String): A SpaceRole id or a
    // SpaceMember `id`. `targetType` (String): `"role"` or `"member"`."

    use serde_json::json;

    fn overrides(v: serde_json::Value) -> Vec<ChannelPermission> {
        serde_json::from_value(v).expect("hand-written ChannelPermission[]")
    }

    fn set(items: &[&str]) -> HashSet<ChatPermission> {
        serde_json::from_value(json!(items)).expect("hand-written permission names")
    }

    fn resolve(base: &[&str], roles: &[&str], o: serde_json::Value) -> HashSet<ChatPermission> {
        let role_ids: HashSet<&str> = roles.iter().copied().collect();
        resolve_channel_permissions(set(base), "s1", "m1", &role_ids, &overrides(o))
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

    // §ChannelPermission `targetId` (jmap-chat-spec 29e9a41, L853): "A
    // record with `targetType: "role"` whose `targetId` is the id of the
    // Space itself addresses the implicit `@everyone` role". Step 2 (L1706)
    // applies "the record for `@everyone`, whose `targetId` is the Space
    // id", so it matches a member that holds no SpaceRole.
    #[test]
    fn space_id_role_record_addresses_everyone() {
        let o = json!([
            { "targetId": "s1", "targetType": "role", "allow": ["pin"], "deny": ["send"] }
        ]);
        assert_eq!(resolve(&["view", "send"], &[], o), set(&["view", "pin"]));
    }

    // The `@everyone` record is role-targeted, so it is applied in steps
    // 2 and 3, before the member-targeted steps 4 and 5: a member allow
    // beats an `@everyone` deny.
    #[test]
    fn member_allow_overrides_everyone_deny() {
        let o = json!([
            { "targetId": "s1", "targetType": "role", "allow": [], "deny": ["send"] },
            { "targetId": "m1", "targetType": "member", "allow": ["send"], "deny": [] }
        ]);
        assert_eq!(resolve(&["view", "send"], &[], o), set(&["view", "send"]));
    }

    // Only `targetType: "role"` with the Space id addresses `@everyone`;
    // a member-targeted record naming the Space id matches no member.
    #[test]
    fn space_id_member_record_matches_nothing() {
        let o = json!([
            { "targetId": "s1", "targetType": "member", "allow": [], "deny": ["send"] }
        ]);
        assert_eq!(resolve(&["view", "send"], &[], o), set(&["view", "send"]));
    }

    // §SpaceRole (jmap-chat-spec 33702a5, L809): "Servers MUST ignore
    // unrecognized permission names." A `ChatPermission::Other` spelling
    // of a known name is matched on its wire string when denied, and is
    // not the known permission when allowed: it can only fail closed.
    #[test]
    fn forged_other_spelling_denies_but_never_grants() {
        let mut o = overrides(json!([
            { "targetId": "m1", "targetType": "member", "allow": [], "deny": [] }
        ]));
        o[0].deny.push(ChatPermission::Other("send".to_owned()));
        o[0].allow.push(ChatPermission::Other("pin".to_owned()));
        let role_ids = HashSet::new();
        let perms = resolve_channel_permissions(set(&["view", "send"]), "s1", "m1", &role_ids, &o);
        assert!(!perms.contains(&ChatPermission::Send));
        assert!(!perms.contains(&ChatPermission::Pin));
        assert!(perms.contains(&ChatPermission::View));
    }
}
