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
    pub id: Id,
    pub role: String,
    pub joined_at: UTCDate,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invited_by: Option<Id>,
}

/// A per-channel permission override entry.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPermission {
    pub target_id: Id,
    pub target_type: String,
    pub allow: Vec<String>,
    pub deny: Vec<String>,
}

/// A JMAP Chat object.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chat {
    pub id: Id,
    pub kind: ChatKind,
    pub created_at: UTCDate,
    pub unread_count: u64,
    pub pinned_message_ids: Vec<Id>,
    pub muted: bool,
    pub receive_typing_indicators: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact_id: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_blob_id: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub members: Option<Vec<ChatMember>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_id: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_id: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slow_mode_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_overrides: Option<Vec<ChannelPermission>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_message_at: Option<UTCDate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mute_until: Option<UTCDate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt_sharing: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_expiry_seconds: Option<u64>,
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
        };
        let output = serde_json::to_string(&member).expect("serialize ChatMember");
        assert!(!output.contains("invitedBy"), "invitedBy must be absent");
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
