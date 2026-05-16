//! Space (server-like container), categories, roles, members, invites, and bans.

use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

/// A role that can be assigned to members within a Space.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceRole {
    /// The `id` property (draft-atwood-jmap-chat-00 §4.12).
    pub id: Id,
    /// The `name` property (draft-atwood-jmap-chat-00 §4.12).
    pub name: String,
    /// The `color` property (draft-atwood-jmap-chat-00 §4.12).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// The `permissions` property (draft-atwood-jmap-chat-00 §4.12).
    ///
    /// Named permissions this role grants. Drawn from the
    /// spec-enumerated permission vocabulary exported as
    /// [`crate::vocabulary::SPEC_PERMISSION_NAMES`]. Servers MUST
    /// ignore unrecognized names per the draft; consumers SHOULD
    /// validate caller-supplied input against the const list to
    /// surface typos at the boundary.
    pub permissions: Vec<String>,
    /// The `position` property (draft-atwood-jmap-chat-00 §4.12).
    pub position: u64,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A member of a Space and their assigned roles.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceMember {
    /// The `id` property (draft-atwood-jmap-chat-00 §4.13).
    pub id: Id,
    /// The `roleIds` property (draft-atwood-jmap-chat-00 §4.13).
    pub role_ids: Vec<Id>,
    /// The `nick` property (draft-atwood-jmap-chat-00 §4.13).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nick: Option<String>,
    /// The `joinedAt` property (draft-atwood-jmap-chat-00 §4.13).
    pub joined_at: UTCDate,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A category grouping channels within a Space.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    /// The `id` property (draft-atwood-jmap-chat-00 §4.14).
    pub id: Id,
    /// The `name` property (draft-atwood-jmap-chat-00 §4.14).
    pub name: String,
    /// The `position` property (draft-atwood-jmap-chat-00 §4.14).
    pub position: u64,
    /// The `channelIds` property (draft-atwood-jmap-chat-00 §4.14).
    pub channel_ids: Vec<Id>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A Space is a server-like container holding channels, members, and roles.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Space {
    /// The `id` property (draft-atwood-jmap-chat-00 §4.16).
    pub id: Id,
    /// The `name` property (draft-atwood-jmap-chat-00 §4.16).
    pub name: String,
    /// The `description` property (draft-atwood-jmap-chat-00 §4.16).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The `iconBlobId` property (draft-atwood-jmap-chat-00 §4.16).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_blob_id: Option<Id>,
    /// The `roles` property (draft-atwood-jmap-chat-00 §4.16).
    pub roles: Vec<SpaceRole>,
    /// The `members` property (draft-atwood-jmap-chat-00 §4.16).
    pub members: Vec<SpaceMember>,
    /// The `categories` property (draft-atwood-jmap-chat-00 §4.16).
    pub categories: Vec<Category>,
    /// The `uncategorizedChannelIds` property (draft-atwood-jmap-chat-00 §4.16).
    pub uncategorized_channel_ids: Vec<Id>,
    /// The `createdAt` property (draft-atwood-jmap-chat-00 §4.16).
    pub created_at: UTCDate,
    /// The `isPublic` property (draft-atwood-jmap-chat-00 §4.16).
    pub is_public: bool,
    /// The `isPubliclyPreviewable` property (draft-atwood-jmap-chat-00 §4.16).
    pub is_publicly_previewable: bool,
    /// The `memberCount` property (draft-atwood-jmap-chat-00 §4.16).
    pub member_count: u64,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// An invite code allowing others to join a Space.
///
/// # Debug redaction
///
/// The `code` field is a secret credential (draft-atwood-jmap-chat-00 §4.18 —
/// anyone with the code can join the Space). The `Debug` impl on this type
/// redacts `code` to `"[REDACTED]"` so an accidental `{:?}`-format in an
/// application log, tracing span, or test fixture cannot leak it. To inspect
/// the value programmatically, access the `code` field directly.
#[non_exhaustive]
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceInvite {
    /// The `id` property (draft-atwood-jmap-chat-00 §4.18).
    pub id: Id,
    /// The `code` property (draft-atwood-jmap-chat-00 §4.18).
    ///
    /// Unguessable secret — the bearer can redeem it to join the Space.
    /// Redacted by the [`std::fmt::Debug`] impl on this struct.
    ///
    /// # Constant-time comparison
    ///
    /// This field is a credential. Any code that compares a stored `code`
    /// against an attacker-supplied value MUST use a constant-time
    /// equality check (e.g. `subtle::ConstantTimeEq::ct_eq` on the byte
    /// slices) to defeat byte-by-byte timing oracles. A plain
    /// `String == String` short-circuits at the first mismatch and is
    /// exploitable over the network despite jitter. See
    /// `jmap-chat-server::space::handle_space_join` for the canonical
    /// usage pattern.
    pub code: String,
    /// The `spaceId` property (draft-atwood-jmap-chat-00 §4.18).
    pub space_id: Id,
    /// The `createdBy` property (draft-atwood-jmap-chat-00 §4.18).
    pub created_by: Id,
    /// The `uses` property (draft-atwood-jmap-chat-00 §4.18).
    pub uses: u64,
    /// The `createdAt` property (draft-atwood-jmap-chat-00 §4.18).
    pub created_at: UTCDate,
    /// The `defaultChannelId` property (draft-atwood-jmap-chat-00 §4.18).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_channel_id: Option<Id>,
    /// The `expiresAt` property (draft-atwood-jmap-chat-00 §4.18).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<UTCDate>,
    /// The `maxUses` property (draft-atwood-jmap-chat-00 §4.18).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_uses: Option<u64>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl SpaceInvite {
    /// Construct a [`SpaceInvite`] from its required and optional fields.
    ///
    /// `default_channel_id`, `expires_at`, and `max_uses` default to `None`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: Id,
        code: impl Into<String>,
        space_id: Id,
        created_by: Id,
        uses: u64,
        created_at: UTCDate,
        default_channel_id: Option<Id>,
        expires_at: Option<UTCDate>,
        max_uses: Option<u64>,
    ) -> Self {
        Self {
            id,
            code: code.into(),
            space_id,
            created_by,
            uses,
            created_at,
            default_channel_id,
            expires_at,
            max_uses,
            extra: serde_json::Map::new(),
        }
    }
}

impl std::fmt::Debug for SpaceInvite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpaceInvite")
            .field("id", &self.id)
            .field("code", &"[REDACTED]")
            .field("space_id", &self.space_id)
            .field("created_by", &self.created_by)
            .field("uses", &self.uses)
            .field("created_at", &self.created_at)
            .field("default_channel_id", &self.default_channel_id)
            .field("expires_at", &self.expires_at)
            .field("max_uses", &self.max_uses)
            .field("extra", &self.extra)
            .finish()
    }
}

/// A ban preventing a user from accessing a Space.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceBan {
    /// The `id` property (draft-atwood-jmap-chat-00 §4.19).
    pub id: Id,
    /// The `spaceId` property (draft-atwood-jmap-chat-00 §4.19).
    pub space_id: Id,
    /// The `userId` property (draft-atwood-jmap-chat-00 §4.19).
    pub user_id: Id,
    /// The `bannedBy` property (draft-atwood-jmap-chat-00 §4.19).
    pub banned_by: Id,
    /// The `createdAt` property (draft-atwood-jmap-chat-00 §4.19).
    pub created_at: UTCDate,
    /// The `reason` property (draft-atwood-jmap-chat-00 §4.19).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The `expiresAt` property (draft-atwood-jmap-chat-00 §4.19).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<UTCDate>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl SpaceBan {
    /// Construct a [`SpaceBan`] from its required server-set and client-provided fields.
    ///
    /// `reason` and `expires_at` default to `None`.
    pub fn new(id: Id, space_id: Id, user_id: Id, banned_by: Id, created_at: UTCDate) -> Self {
        Self {
            id,
            space_id,
            user_id,
            banned_by,
            created_at,
            reason: None,
            expires_at: None,
            extra: serde_json::Map::new(),
        }
    }
}

impl Space {
    /// Construct a [`Space`] from its required fields.
    ///
    /// `description` and `icon_blob_id` default to `None`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: Id,
        name: impl Into<String>,
        roles: Vec<SpaceRole>,
        members: Vec<SpaceMember>,
        categories: Vec<Category>,
        uncategorized_channel_ids: Vec<Id>,
        created_at: UTCDate,
        is_public: bool,
        is_publicly_previewable: bool,
        member_count: u64,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            roles,
            members,
            categories,
            uncategorized_channel_ids,
            created_at,
            is_public,
            is_publicly_previewable,
            member_count,
            description: None,
            icon_blob_id: None,
            extra: serde_json::Map::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Oracle: spec §Space — required Vec fields deserialize as empty arrays, not None.
    #[test]
    fn space_deser() {
        let json = r#"{
            "id": "s1",
            "name": "My Space",
            "roles": [],
            "members": [],
            "categories": [],
            "uncategorizedChannelIds": [],
            "createdAt": "2024-01-01T00:00:00Z",
            "isPublic": false,
            "isPubliclyPreviewable": false,
            "memberCount": 0
        }"#;
        let space: Space = serde_json::from_str(json).expect("deserialize Space");
        assert_eq!(space.roles, vec![]);
        assert_eq!(space.members, vec![]);
        assert_eq!(space.categories, vec![]);
        assert_eq!(space.uncategorized_channel_ids, Vec::<Id>::new());
        assert_eq!(space.member_count, 0);
        assert!(!space.is_public);
    }

    // Oracle: spec §Space.categories — channel_ids within a category round-trips.
    #[test]
    fn space_with_categories() {
        let json = r#"{
            "id": "s2",
            "name": "Guild",
            "roles": [],
            "members": [],
            "categories": [
                {
                    "id": "cat1",
                    "name": "General",
                    "position": 0,
                    "channelIds": ["ch1", "ch2", "ch3"]
                }
            ],
            "uncategorizedChannelIds": [],
            "createdAt": "2024-06-01T12:00:00Z",
            "isPublic": true,
            "isPubliclyPreviewable": true,
            "memberCount": 42
        }"#;
        let space: Space = serde_json::from_str(json).expect("deserialize Space with categories");
        assert_eq!(space.categories.len(), 1);
        assert_eq!(space.categories[0].channel_ids.len(), 3);
    }

    // Oracle: serde serialization contract — required Vec fields appear as [] in output.
    #[test]
    fn space_ser_required_vecs_present() {
        let space = Space {
            id: Id::from("s3"),
            name: "Empty Space".to_owned(),
            description: None,
            icon_blob_id: None,
            roles: vec![],
            members: vec![],
            categories: vec![],
            uncategorized_channel_ids: vec![],
            created_at: UTCDate::from("2024-01-01T00:00:00Z"),
            is_public: false,
            is_publicly_previewable: false,
            member_count: 0,
            extra: serde_json::Map::new(),
        };
        let json = serde_json::to_string(&space).expect("serialize Space");
        assert!(json.contains("\"roles\":[]"), "roles must be present as []");
        assert!(
            json.contains("\"members\":[]"),
            "members must be present as []"
        );
        assert!(
            json.contains("\"categories\":[]"),
            "categories must be present as []"
        );
    }

    // Oracle: spec §SpaceInvite — all fields round-trip through JSON.
    #[test]
    fn space_invite_roundtrip() {
        let json = r#"{
            "id": "inv1",
            "code": "ABCD1234",
            "spaceId": "s1",
            "createdBy": "u1",
            "uses": 5,
            "createdAt": "2024-03-15T10:00:00Z",
            "defaultChannelId": "ch1",
            "expiresAt": "2024-12-31T23:59:59Z",
            "maxUses": 100
        }"#;
        let invite: SpaceInvite = serde_json::from_str(json).expect("deserialize SpaceInvite");
        let re_json = serde_json::to_string(&invite).expect("serialize SpaceInvite");
        let invite2: SpaceInvite =
            serde_json::from_str(&re_json).expect("re-deserialize SpaceInvite");
        assert_eq!(invite, invite2);
        assert_eq!(invite.code, "ABCD1234");
        assert_eq!(invite.uses, 5);
        assert_eq!(invite.max_uses, Some(100));
        assert!(invite.default_channel_id.is_some());
        assert!(invite.expires_at.is_some());
    }

    // Oracle: SpaceInvite Debug must NOT contain the raw `code` secret. The
    // canary is a self-defined literal under the test's control, never derived
    // from SpaceInvite's internal state — same tripwire shape as the
    // BearerAuth/BasicAuth redaction tests in jmap-base-client::auth.
    //
    // draft-atwood-jmap-chat-00 §4.18 defines the `code` field as the
    // unguessable bearer credential for Space/join, so a Debug-format leak
    // is a confidentiality regression.
    #[test]
    fn space_invite_debug_does_not_leak_code() {
        const CANARY: &str = "CANARY-INVITE-CODE-DO-NOT-LEAK-9F8E7D";
        let invite = SpaceInvite::new(
            Id::from("inv-canary"),
            CANARY,
            Id::from("s-canary"),
            Id::from("u-canary"),
            0,
            UTCDate::from("2024-01-01T00:00:00Z"),
            None,
            None,
            None,
        );
        let dbg = format!("{invite:?}");
        assert!(
            !dbg.contains(CANARY),
            "SpaceInvite Debug must not contain the raw code; got: {dbg}"
        );
        // Sanity: other fields still appear in Debug output (we only
        // suppressed `code`, not the whole struct).
        assert!(
            dbg.contains("inv-canary"),
            "SpaceInvite Debug must still expose non-secret id; got: {dbg}"
        );
    }

    // Oracle: spec §SpaceBan — reason field is optional and absent means None.
    #[test]
    fn space_ban_no_reason() {
        let json = r#"{
            "id": "ban1",
            "spaceId": "s1",
            "userId": "u2",
            "bannedBy": "u1",
            "createdAt": "2024-02-20T08:00:00Z"
        }"#;
        let ban: SpaceBan = serde_json::from_str(json).expect("deserialize SpaceBan");
        assert_eq!(ban.reason, None);
        assert_eq!(ban.expires_at, None);
    }

    // ── Extras-preservation policy tests (JMAP-lbdy.3) ───────────────────

    /// `SpaceRole.extra` captures vendor fields and preserves them.
    #[test]
    fn space_role_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "id": "r1",
            "name": "admin",
            "permissions": ["all"],
            "position": 0,
            "acmeCorpHidden": true
        });
        let r: SpaceRole = serde_json::from_value(raw).unwrap();
        assert_eq!(
            r.extra.get("acmeCorpHidden").and_then(|v| v.as_bool()),
            Some(true)
        );
        let back = serde_json::to_value(&r).unwrap();
        assert_eq!(back["acmeCorpHidden"], true);
    }

    /// `SpaceMember.extra` captures vendor fields and preserves them.
    #[test]
    fn space_member_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "id": "u1",
            "roleIds": ["r1"],
            "joinedAt": "2026-01-01T00:00:00Z",
            "acmeCorpInviteCode": "x"
        });
        let m: SpaceMember = serde_json::from_value(raw).unwrap();
        assert_eq!(
            m.extra.get("acmeCorpInviteCode").and_then(|v| v.as_str()),
            Some("x")
        );
        let back = serde_json::to_value(&m).unwrap();
        assert_eq!(back["acmeCorpInviteCode"], "x");
    }

    /// `Category.extra` captures vendor fields and preserves them.
    #[test]
    fn category_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "id": "cat1",
            "name": "General",
            "position": 0,
            "channelIds": ["c1"],
            "acmeCorpCollapsedByDefault": true
        });
        let c: Category = serde_json::from_value(raw).unwrap();
        assert_eq!(
            c.extra
                .get("acmeCorpCollapsedByDefault")
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        let back = serde_json::to_value(&c).unwrap();
        assert_eq!(back["acmeCorpCollapsedByDefault"], true);
    }

    /// `Space.extra` captures vendor fields and preserves them across
    /// deserialize/serialize round-trip.
    #[test]
    fn space_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "id": "s1",
            "name": "My Space",
            "roles": [],
            "members": [],
            "categories": [],
            "uncategorizedChannelIds": [],
            "createdAt": "2024-01-01T00:00:00Z",
            "isPublic": false,
            "isPubliclyPreviewable": false,
            "memberCount": 0,
            "acmeCorpBranding": {"primaryColor": "#abcdef"}
        });
        let s: Space = serde_json::from_value(raw).unwrap();
        assert_eq!(
            s.extra
                .get("acmeCorpBranding")
                .and_then(|v| v["primaryColor"].as_str()),
            Some("#abcdef")
        );
        let back = serde_json::to_value(&s).unwrap();
        assert_eq!(back["acmeCorpBranding"]["primaryColor"], "#abcdef");
    }

    /// `SpaceInvite.extra` captures vendor fields and preserves them.
    #[test]
    fn space_invite_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "id": "inv1",
            "code": "ABCD",
            "spaceId": "s1",
            "createdBy": "u1",
            "uses": 0,
            "createdAt": "2024-01-01T00:00:00Z",
            "acmeCorpInviteSource": "marketing-page"
        });
        let inv: SpaceInvite = serde_json::from_value(raw).unwrap();
        assert_eq!(
            inv.extra
                .get("acmeCorpInviteSource")
                .and_then(|v| v.as_str()),
            Some("marketing-page")
        );
        let back = serde_json::to_value(&inv).unwrap();
        assert_eq!(back["acmeCorpInviteSource"], "marketing-page");
    }

    /// `SpaceBan.extra` captures vendor fields and preserves them.
    #[test]
    fn space_ban_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "id": "ban1",
            "spaceId": "s1",
            "userId": "u2",
            "bannedBy": "u1",
            "createdAt": "2024-01-01T00:00:00Z",
            "acmeCorpCaseId": "abuse-42"
        });
        let b: SpaceBan = serde_json::from_value(raw).unwrap();
        assert_eq!(
            b.extra.get("acmeCorpCaseId").and_then(|v| v.as_str()),
            Some("abuse-42")
        );
        let back = serde_json::to_value(&b).unwrap();
        assert_eq!(back["acmeCorpCaseId"], "abuse-42");
    }

    // Oracle: spec §SpaceBan — all fields round-trip through JSON.
    #[test]
    fn space_ban_roundtrip() {
        let json = r#"{
            "id": "ban2",
            "spaceId": "s1",
            "userId": "u3",
            "bannedBy": "u1",
            "createdAt": "2024-02-21T09:00:00Z",
            "reason": "Spam",
            "expiresAt": "2024-03-21T09:00:00Z"
        }"#;
        let ban: SpaceBan = serde_json::from_str(json).expect("deserialize SpaceBan");
        let re_json = serde_json::to_string(&ban).expect("serialize SpaceBan");
        let ban2: SpaceBan = serde_json::from_str(&re_json).expect("re-deserialize SpaceBan");
        assert_eq!(ban, ban2);
        assert_eq!(ban.reason, Some("Spam".to_owned()));
        assert!(ban.expires_at.is_some());
    }
}
