//! Chat conversation object and supporting types.

use jmap_types::{impl_string_enum, Id, UTCDate};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The kind of a [`Chat`] conversation.
///
/// The spec defines three kinds. `Other` preserves any future value
/// for round-trip fidelity without breaking deserialization.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChatKind {
    /// One-to-one conversation between two participants.
    Direct,
    /// Multi-party conversation without a containing Space.
    Group,
    /// A named channel inside a [`crate::Space`].
    Channel,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(ChatKind, "a chat kind string",
    "direct" => Direct,
    "group" => Group,
    "channel" => Channel,
);

/// The `role` of a [`ChatMember`] (draft-atwood-jmap-chat-00 §ChatMember).
///
/// The draft defines two values: `"admin"` and `"member"`.
/// `Other(String)` preserves any unrecognized value for lossless round-trip.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum ChatMemberRole {
    /// Group or channel administrator with management permissions.
    Admin,
    /// Regular member.
    Member,
    /// Catch-all for any unrecognized wire value from a future spec version.
    ///
    /// **Forging caveat**: constructing `ChatMemberRole::Other("admin".into())`
    /// produces a value that is unequal to `ChatMemberRole::Admin` on
    /// `PartialEq` but serialises to the same wire string `"admin"`
    /// and round-trips back to `ChatMemberRole::Admin`. Reserve
    /// `Other(s)` for genuinely unrecognised wire strings; compare
    /// wire equality via `as_str()`, not `PartialEq`.
    Other(String),
}

impl ChatMemberRole {
    /// The canonical wire string for this role.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Admin => "admin",
            Self::Member => "member",
            Self::Other(s) => s.as_str(),
        }
    }
}

impl_string_enum!(ChatMemberRole, "a ChatMemberRole wire string",
    "admin"  => Admin,
    "member" => Member,
);

/// A member of a [`Chat`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMember {
    /// The `id` property (draft-atwood-jmap-chat-00 §4.9).
    pub id: Id,
    /// The `role` property (draft-atwood-jmap-chat-00 §4.9).
    ///
    /// Wire-observable values are enumerated by the draft as
    /// `"admin"` or `"member"`; any other string deserializes to
    /// [`ChatMemberRole::Other`]. Servers MAY designate additional internal
    /// principals as having admin-equivalent authority; those still
    /// appear as `"admin"` on the wire.
    pub role: ChatMemberRole,
    /// The `joinedAt` property (draft-atwood-jmap-chat-00 §4.9).
    pub joined_at: UTCDate,
    /// The `invitedBy` property (draft-atwood-jmap-chat-00 §4.9).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invited_by: Option<Id>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl ChatMember {
    /// Construct a [`ChatMember`] from its required fields.
    ///
    /// `invited_by` defaults to `None` and `extra` to an empty map.
    pub fn new(id: Id, role: ChatMemberRole, joined_at: UTCDate) -> Self {
        Self {
            id,
            role,
            joined_at,
            invited_by: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// The `targetType` of a [`ChannelPermission`] (draft-atwood-jmap-chat-00
/// §ChannelPermission: "`"role"` or `"member"`").
///
/// Any other string deserializes to [`Other`](Self::Other) and serializes
/// back unchanged.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChannelPermissionTargetType {
    /// `"role"`: `targetId` is a SpaceRole id.
    Role,
    /// `"member"`: `targetId` is a SpaceMember `id`.
    Member,
    /// Any value this version does not recognize.
    ///
    /// **Forging caveat**: `Other("role".into())` is unequal to
    /// [`Role`](Self::Role) on `PartialEq` but serializes to the same wire
    /// string. Reserve `Other(s)` for unrecognized strings; compare wire
    /// equality via [`as_str`](Self::as_str).
    Other(String),
}

impl ChannelPermissionTargetType {
    /// The wire string for this target type.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Role => "role",
            Self::Member => "member",
            Self::Other(s) => s.as_str(),
        }
    }
}

impl_string_enum!(ChannelPermissionTargetType, "a ChannelPermission targetType string",
    "role" => Role,
    "member" => Member,
);

/// A per-channel permission override entry.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPermission {
    /// The `targetId` property (draft-atwood-jmap-chat-00 §4.15).
    pub target_id: Id,
    /// The `targetType` property (draft-atwood-jmap-chat-00 §4.15).
    ///
    /// Spec-enumerated values: `"role"` or `"member"`.
    pub target_type: ChannelPermissionTargetType,
    /// The `allow` property (draft-atwood-jmap-chat-00 §4.15).
    ///
    /// Permissions explicitly granted in this channel, overriding the
    /// Space-level role defaults. Unrecognized names deserialize to
    /// [`ChatPermission::Other`](crate::ChatPermission::Other); servers
    /// MUST ignore them.
    pub allow: Vec<crate::ChatPermission>,
    /// The `deny` property (draft-atwood-jmap-chat-00 §4.15).
    ///
    /// Same vocabulary contract as [`allow`](Self::allow).
    pub deny: Vec<crate::ChatPermission>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A JMAP Chat object.
///
/// For a `"channel"` Chat, `slowModeSeconds` and `permissionOverrides` are
/// always serialized, as their defaults `0` and `[]` when `None`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", remote = "Self")]
pub struct Chat {
    /// The `id` property (draft-atwood-jmap-chat-00 §4.10).
    pub id: Id,
    /// The `kind` property (draft-atwood-jmap-chat-00 §4.10).
    pub kind: ChatKind,
    /// The `createdAt` property (draft-atwood-jmap-chat-00 §4.10).
    pub created_at: UTCDate,
    /// The `unreadCount` property (draft-atwood-jmap-chat-00 §4.10).
    pub unread_count: u64,
    /// The `pinnedMessageIds` property (draft-atwood-jmap-chat-00 §4.10).
    /// Empty by default.
    #[serde(default)]
    pub pinned_message_ids: Vec<Id>,
    /// The `muted` property (draft-atwood-jmap-chat-00 §4.10).
    #[serde(default)]
    pub muted: bool,
    /// The `receiveTypingIndicators` property (draft-atwood-jmap-chat-00 §4.10).
    #[serde(default = "crate::default_true")]
    pub receive_typing_indicators: bool,

    /// The `contactId` property (draft-atwood-jmap-chat-00 §4.10).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact_id: Option<Id>,
    /// The `name` property (draft-atwood-jmap-chat-00 §4.10).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The `description` property (draft-atwood-jmap-chat-00 §4.10).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The `avatarBlobId` property (draft-atwood-jmap-chat-00 §4.10).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_blob_id: Option<Id>,
    /// The `members` property (draft-atwood-jmap-chat-00 §4.10).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub members: Option<Vec<ChatMember>>,
    /// The `spaceId` property (draft-atwood-jmap-chat-00 §4.10).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_id: Option<Id>,
    /// The `categoryId` property (draft-atwood-jmap-chat-00 §4.10).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_id: Option<Id>,
    /// The `position` property (draft-atwood-jmap-chat-00 §4.10).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<u64>,
    /// The `topic` property (draft-atwood-jmap-chat-00 §4.10).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    /// The `slowModeSeconds` property (draft-atwood-jmap-chat-00 §4.10).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slow_mode_seconds: Option<u64>,
    /// The `permissionOverrides` property (draft-atwood-jmap-chat-00 §4.10).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_overrides: Option<Vec<ChannelPermission>>,
    /// The `lastMessageAt` property (draft-atwood-jmap-chat-00 §4.10).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_message_at: Option<UTCDate>,
    /// The `muteUntil` property (draft-atwood-jmap-chat-00 §4.10).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mute_until: Option<UTCDate>,
    /// The `receiptSharing` property (draft-atwood-jmap-chat-00 §4.10).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt_sharing: Option<bool>,
    /// The `messageExpirySeconds` property (draft-atwood-jmap-chat-00 §4.10).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_expiry_seconds: Option<u64>,
    /// The server-set `closedAt` property (draft-atwood-jmap-chat-00
    /// §4.10). Group chats only; absent while the group is open.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<UTCDate>,
    /// The server-set `activeCallId` property (draft-atwood-jmap-chat-00
    /// §4.10): the id of the active VTCCall bound to this Chat. Typed
    /// `String|null`, so `None` serializes as `null`, and it is not a
    /// `Chat/set` patch key.
    #[serde(default)]
    pub active_call_id: Option<Id>,
    /// The server-set `activeCallAccountId` property
    /// (draft-atwood-jmap-chat-00 §4.10): the id of the account hosting
    /// the VTCCall named by `activeCallId`. Typed `Id|null`, non-null
    /// exactly when `activeCallId` is, and not a `Chat/set` patch key.
    #[serde(default)]
    pub active_call_account_id: Option<Id>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Serialize for Chat {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // draft-atwood-jmap-chat-00 §4.10: channel `slowModeSeconds` has
        // "Default: `0`" and `permissionOverrides` is "Empty by default".
        if self.kind == ChatKind::Channel
            && (self.slow_mode_seconds.is_none() || self.permission_overrides.is_none())
        {
            let mut chat = self.clone();
            chat.slow_mode_seconds.get_or_insert(0);
            chat.permission_overrides.get_or_insert_with(Vec::new);
            return Chat::serialize(&chat, serializer);
        }
        Chat::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for Chat {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Chat::deserialize(deserializer)
    }
}

impl Chat {
    /// Construct a [`Chat`] from its required fields.
    ///
    /// All optional fields default to `None`.
    pub fn new(
        id: Id,
        kind: ChatKind,
        created_at: UTCDate,
        unread_count: u64,
        pinned_message_ids: Vec<Id>,
        muted: bool,
        receive_typing_indicators: bool,
    ) -> Self {
        Self {
            id,
            kind,
            created_at,
            unread_count,
            pinned_message_ids,
            muted,
            receive_typing_indicators,
            contact_id: None,
            name: None,
            description: None,
            avatar_blob_id: None,
            members: None,
            space_id: None,
            category_id: None,
            position: None,
            topic: None,
            slow_mode_seconds: None,
            permission_overrides: None,
            last_message_at: None,
            mute_until: None,
            receipt_sharing: None,
            message_expiry_seconds: None,
            closed_at: None,
            active_call_id: None,
            active_call_account_id: None,
            extra: serde_json::Map::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Oracle: hand-written JSON matching spec field names (camelCase).
    // Verifies required fields deserialize and absent optional fields are None.
    #[test]
    fn chat_direct_deser() {
        let json = r#"{"id":"c1","kind":"direct","contactId":"u1","createdAt":"2026-01-01T00:00:00Z","unreadCount":0,"pinnedMessageIds":[],"muted":false,"receiveTypingIndicators":true}"#;
        let chat: Chat = serde_json::from_str(json).expect("deserialize Chat");
        assert_eq!(chat.id.as_ref(), "c1");
        assert_eq!(chat.kind, ChatKind::Direct);
        assert_eq!(chat.contact_id.as_ref().map(|id| id.as_ref()), Some("u1"));
        assert_eq!(chat.created_at.as_ref(), "2026-01-01T00:00:00Z");
        assert!(chat.name.is_none());
        assert!(chat.members.is_none());
    }

    // Oracle: hand-written JSON with channel-specific fields.
    // Verifies space_id and permission_overrides deserialize correctly.
    #[test]
    fn chat_channel_deser() {
        let json = r#"{
            "id": "ch1",
            "kind": "channel",
            "spaceId": "sp1",
            "createdAt": "2026-01-15T12:00:00Z",
            "unreadCount": 3,
            "pinnedMessageIds": [],
            "muted": false,
            "receiveTypingIndicators": false,
            "permissionOverrides": [
                {"targetId": "r1", "targetType": "role", "allow": ["send_message"], "deny": []}
            ]
        }"#;
        let chat: Chat = serde_json::from_str(json).expect("deserialize channel Chat");
        assert_eq!(chat.space_id.as_ref().map(|id| id.as_ref()), Some("sp1"));
        let overrides = chat
            .permission_overrides
            .as_ref()
            .expect("permission_overrides present");
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0].target_id.as_ref(), "r1");
        assert_eq!(overrides[0].target_type, ChannelPermissionTargetType::Role);
        assert_eq!(
            overrides[0].allow,
            vec![crate::ChatPermission::Other("send_message".to_owned())]
        );
        assert!(overrides[0].deny.is_empty());
    }

    // Oracle: optional fields not set must be absent from JSON output.
    // Constructs a minimal Chat, serializes, and checks key absence.
    #[test]
    fn chat_ser_omits_none() {
        let json = r#"{"id":"c2","kind":"direct","createdAt":"2026-02-01T00:00:00Z","unreadCount":0,"pinnedMessageIds":[],"muted":false,"receiveTypingIndicators":false}"#;
        let chat: Chat = serde_json::from_str(json).expect("deserialize minimal Chat");
        let output = serde_json::to_string(&chat).expect("serialize Chat");
        assert!(!output.contains("contactId"), "contactId must be absent");
        assert!(!output.contains("members"), "members must be absent");
        assert!(!output.contains("spaceId"), "spaceId must be absent");
    }

    // Oracle: hand-written JSON with invitedBy present.
    // Verifies invited_by deserializes to Some(Id).
    #[test]
    fn chat_member_with_invited_by() {
        let json =
            r#"{"id":"m1","role":"member","joinedAt":"2026-01-10T08:00:00Z","invitedBy":"m0"}"#;
        let member: ChatMember = serde_json::from_str(json).expect("deserialize ChatMember");
        assert_eq!(member.invited_by.as_ref().map(|id| id.as_ref()), Some("m0"));
    }

    // Oracle: draft-atwood-jmap-chat-00 §ChatMember: `role` (String):
    // "Either `"admin"` or `"member"`."
    #[test]
    fn chat_member_role_spec_values_round_trip() {
        for (wire, want) in [
            ("admin", ChatMemberRole::Admin),
            ("member", ChatMemberRole::Member),
        ] {
            let raw = serde_json::json!({
                "id": "m1",
                "role": wire,
                "joinedAt": "2026-01-10T08:00:00Z"
            });
            let member: ChatMember = serde_json::from_value(raw.clone()).expect("deserialize");
            assert_eq!(member.role, want);
            assert_eq!(member.role.as_str(), wire);
            assert_eq!(serde_json::to_value(&member).expect("serialize"), raw);
        }
    }

    // Oracle: `"moderator"` is not one of the draft-atwood-jmap-chat-00
    // §ChatMember role values {"admin", "member"}; it must survive a
    // round-trip as `Other` per the workspace extras-preservation policy.
    #[test]
    fn chat_member_role_unknown_round_trips_via_other() {
        let raw = r#""moderator""#;
        let parsed: ChatMemberRole = serde_json::from_str(raw).expect("must deserialize");
        assert_eq!(parsed, ChatMemberRole::Other("moderator".to_owned()));
        assert_eq!(serde_json::to_string(&parsed).unwrap(), raw);
    }

    // Oracle: ChatMember without invited_by must not emit "invitedBy" key.
    #[test]
    fn chat_member_no_invited_by() {
        let member = ChatMember {
            id: Id::from("m2"),
            role: ChatMemberRole::Admin,
            joined_at: UTCDate::from("2026-01-11T09:00:00Z"),
            invited_by: None,
            extra: serde_json::Map::new(),
        };
        let output = serde_json::to_string(&member).expect("serialize ChatMember");
        assert!(!output.contains("invitedBy"), "invitedBy must be absent");
    }

    // ── Extras-preservation policy tests (JMAP-lbdy.3) ───────────────────

    /// `ChatMember.extra` captures vendor fields and preserves them.
    #[test]
    fn chat_member_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "id": "m1",
            "role": "member",
            "joinedAt": "2026-01-10T08:00:00Z",
            "acmeCorpInviteSource": "link"
        });
        let m: ChatMember = serde_json::from_value(raw).unwrap();
        assert_eq!(
            m.extra.get("acmeCorpInviteSource").and_then(|v| v.as_str()),
            Some("link")
        );
        let back = serde_json::to_value(&m).unwrap();
        assert_eq!(back["acmeCorpInviteSource"], "link");
    }

    /// `ChannelPermission.extra` captures vendor fields and preserves them.
    #[test]
    fn channel_permission_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "targetId": "r1",
            "targetType": "role",
            "allow": ["send_message"],
            "deny": [],
            "acmeCorpAuditNote": "added-by-script"
        });
        let p: ChannelPermission = serde_json::from_value(raw).unwrap();
        assert_eq!(
            p.extra.get("acmeCorpAuditNote").and_then(|v| v.as_str()),
            Some("added-by-script")
        );
        let back = serde_json::to_value(&p).unwrap();
        assert_eq!(back["acmeCorpAuditNote"], "added-by-script");
    }

    /// `Chat.extra` captures vendor fields and preserves them across
    /// deserialize/serialize round-trip.
    #[test]
    fn chat_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "id": "c1",
            "kind": "direct",
            "createdAt": "2026-01-01T00:00:00Z",
            "unreadCount": 0,
            "pinnedMessageIds": [],
            "muted": false,
            "receiveTypingIndicators": true,
            "acmeCorpRoutingTag": "us-east"
        });
        let c: Chat = serde_json::from_value(raw).unwrap();
        assert_eq!(
            c.extra.get("acmeCorpRoutingTag").and_then(|v| v.as_str()),
            Some("us-east")
        );
        let back = serde_json::to_value(&c).unwrap();
        assert_eq!(back["acmeCorpRoutingTag"], "us-east");
    }

    // Oracle: hand-written JSON round-trips through serialize then deserialize.
    // Equality check proves both directions are consistent.
    #[test]
    fn channel_permission_roundtrip() {
        let json = r#"{"targetId":"role42","targetType":"role","allow":["send_message","manage_channels"],"deny":["ban"]}"#;
        let perm: ChannelPermission =
            serde_json::from_str(json).expect("deserialize ChannelPermission");
        let serialized = serde_json::to_string(&perm).expect("serialize ChannelPermission");
        let perm2: ChannelPermission =
            serde_json::from_str(&serialized).expect("re-deserialize ChannelPermission");
        assert_eq!(perm, perm2);
    }

    /// Oracle: draft-atwood-jmap-chat-00 §Chat Object Fields: `slowModeSeconds`
    /// "**Channel Chats only.** ... Default: `0`." and `permissionOverrides`
    /// "**Channel Chats only.** ... Empty by default."
    #[test]
    fn chat_channel_unset_defaults_serialized() {
        let json = r#"{"id":"ch1","kind":"channel","spaceId":"s1","name":"general","createdAt":"2026-01-01T00:00:00Z","unreadCount":0,"pinnedMessageIds":[]}"#;
        let chat: Chat = serde_json::from_str(json).expect("deserialize Chat");
        let out = serde_json::to_value(&chat).expect("serialize Chat");
        assert_eq!(out["slowModeSeconds"], serde_json::json!(0), "{out}");
        assert_eq!(out["permissionOverrides"], serde_json::json!([]), "{out}");
    }

    /// draft-atwood-jmap-chat-00 §Chat: "`closedAt` (UTCDate, optional,
    /// server-set): **Group chats only.** Time the group closed. Absent
    /// while the group is open."
    #[test]
    fn chat_closed_at_typed_and_absent_while_open() {
        let raw = serde_json::json!({
            "id": "c1",
            "kind": "group",
            "createdAt": "2026-01-01T00:00:00Z",
            "unreadCount": 0,
            "closedAt": "2026-02-01T00:00:00Z"
        });
        let c: Chat = serde_json::from_value(raw).expect("deserialize closed Chat");
        assert_eq!(
            c.closed_at.as_ref().map(AsRef::as_ref),
            Some("2026-02-01T00:00:00Z")
        );
        assert!(c.extra.is_empty(), "closedAt must not land in extra");
        let back = serde_json::to_value(&c).expect("serialize Chat");
        assert_eq!(back["closedAt"], "2026-02-01T00:00:00Z");

        let open = Chat::new(
            Id::from("c2"),
            ChatKind::Group,
            UTCDate::from("2026-01-01T00:00:00Z"),
            0,
            vec![],
            false,
            true,
        );
        let out = serde_json::to_value(&open).expect("serialize Chat");
        assert!(out.get("closedAt").is_none(), "{out}");
    }

    // Oracle: draft-atwood-jmap-chat-00 L567-568: "`activeCallId`
    // (String|null, server-set): ... the id of a VTCCall that is currently
    // active and bound to this Chat ... `null` when no call is active or
    // VTC is not available." Hand-written JSON.
    #[test]
    fn chat_active_call_id_typed_and_null_when_no_call() {
        let raw = serde_json::json!({
            "id": "c1",
            "kind": "group",
            "createdAt": "2026-01-01T00:00:00Z",
            "unreadCount": 0,
            "activeCallId": "call1"
        });
        let c: Chat = serde_json::from_value(raw).expect("deserialize Chat");
        assert_eq!(c.active_call_id.as_ref().map(AsRef::as_ref), Some("call1"));
        assert!(c.extra.is_empty(), "activeCallId must not land in extra");
        let back = serde_json::to_value(&c).expect("serialize Chat");
        assert_eq!(back["activeCallId"], "call1");

        let raw = serde_json::json!({
            "id": "c2",
            "kind": "group",
            "createdAt": "2026-01-01T00:00:00Z",
            "unreadCount": 0,
            "activeCallId": null
        });
        let c: Chat = serde_json::from_value(raw).expect("deserialize Chat");
        assert!(c.active_call_id.is_none());
        assert!(c.extra.is_empty(), "activeCallId must not land in extra");

        let idle = Chat::new(
            Id::from("c3"),
            ChatKind::Group,
            UTCDate::from("2026-01-01T00:00:00Z"),
            0,
            vec![],
            false,
            true,
        );
        let out = serde_json::to_value(&idle).expect("serialize Chat");
        assert_eq!(
            out.get("activeCallId"),
            Some(&serde_json::Value::Null),
            "{out}"
        );
    }

    // Oracle: draft-atwood-jmap-chat-00 L661-662: "`activeCallAccountId`
    // (Id|null, server-set): The id of the account hosting the VTCCall
    // named by `activeCallId` ... it is non-null exactly when
    // `activeCallId` is non-null." Hand-written JSON.
    #[test]
    fn chat_active_call_account_id_typed_and_null_when_no_call() {
        let raw = serde_json::json!({
            "id": "c1",
            "kind": "group",
            "createdAt": "2026-01-01T00:00:00Z",
            "unreadCount": 0,
            "activeCallId": "call1",
            "activeCallAccountId": "shared1"
        });
        let c: Chat = serde_json::from_value(raw).expect("deserialize Chat");
        assert_eq!(
            c.active_call_account_id.as_ref().map(AsRef::as_ref),
            Some("shared1")
        );
        assert!(
            c.extra.is_empty(),
            "activeCallAccountId must not land in extra"
        );
        let back = serde_json::to_value(&c).expect("serialize Chat");
        assert_eq!(back["activeCallAccountId"], "shared1");

        let idle = Chat::new(
            Id::from("c2"),
            ChatKind::Group,
            UTCDate::from("2026-01-01T00:00:00Z"),
            0,
            vec![],
            false,
            true,
        );
        let out = serde_json::to_value(&idle).expect("serialize Chat");
        assert_eq!(
            out.get("activeCallAccountId"),
            Some(&serde_json::Value::Null),
            "{out}"
        );
    }
}
