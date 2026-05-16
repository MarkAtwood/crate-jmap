//! Chat conversation object and supporting types.

use jmap_types::{impl_string_enum, Id, UTCDate};
use serde::{Deserialize, Serialize};

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
    /// `"admin"` or `"member"`; the full list is exported as
    /// [`crate::vocabulary::SPEC_CHAT_MEMBER_ROLES`] for caller-side
    /// validation. Servers MAY designate additional internal
    /// principals as having admin-equivalent authority; those still
    /// appear as `"admin"` on the wire.
    pub role: String,
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

/// A per-channel permission override entry.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPermission {
    /// The `targetId` property (draft-atwood-jmap-chat-00 §4.15).
    pub target_id: Id,
    /// The `targetType` property (draft-atwood-jmap-chat-00 §4.15).
    ///
    /// Spec-enumerated values: `"role"` or `"member"`. Full list
    /// exported as
    /// [`crate::vocabulary::SPEC_CHANNEL_PERMISSION_TARGET_TYPES`].
    pub target_type: String,
    /// The `allow` property (draft-atwood-jmap-chat-00 §4.15).
    ///
    /// Permission names explicitly granted in this channel,
    /// overriding the Space-level role defaults. Drawn from the
    /// spec-enumerated permission vocabulary exported as
    /// [`crate::vocabulary::SPEC_PERMISSION_NAMES`]. Servers MUST
    /// ignore unrecognized names per draft §4.12; consumers
    /// SHOULD validate caller-supplied input against the const
    /// list to surface typos at the boundary.
    pub allow: Vec<String>,
    /// The `deny` property (draft-atwood-jmap-chat-00 §4.15).
    ///
    /// Same vocabulary contract as
    /// [`allow`](Self::allow): drawn from
    /// [`crate::vocabulary::SPEC_PERMISSION_NAMES`].
    pub deny: Vec<String>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A JMAP Chat object.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    pub pinned_message_ids: Vec<Id>,
    /// The `muted` property (draft-atwood-jmap-chat-00 §4.10).
    pub muted: bool,
    /// The `receiveTypingIndicators` property (draft-atwood-jmap-chat-00 §4.10).
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
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
        assert_eq!(overrides[0].target_type, "role");
        assert_eq!(overrides[0].allow, vec!["send_message"]);
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

    // Oracle: ChatMember without invited_by must not emit "invitedBy" key.
    #[test]
    fn chat_member_no_invited_by() {
        let member = ChatMember {
            id: Id::from("m2"),
            role: "admin".to_owned(),
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
}
