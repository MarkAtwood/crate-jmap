//! Space (server-like container), categories, roles, members, invites, and bans.

use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

/// A role that can be assigned to members within a Space.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceRole {
    pub id: Id,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub permissions: Vec<String>,
    pub position: u64,
}

/// A member of a Space and their assigned roles.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceMember {
    pub id: Id,
    pub role_ids: Vec<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nick: Option<String>,
    pub joined_at: UTCDate,
}

/// A category grouping channels within a Space.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: Id,
    pub name: String,
    pub position: u64,
    pub channel_ids: Vec<Id>,
}

/// A Space is a server-like container holding channels, members, and roles.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Space {
    pub id: Id,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_blob_id: Option<Id>,
    pub roles: Vec<SpaceRole>,
    pub members: Vec<SpaceMember>,
    pub categories: Vec<Category>,
    pub uncategorized_channel_ids: Vec<Id>,
    pub created_at: UTCDate,
    pub is_public: bool,
    pub is_publicly_previewable: bool,
    pub member_count: u64,
}

/// An invite code allowing others to join a Space.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceInvite {
    pub id: Id,
    pub code: String,
    pub space_id: Id,
    pub created_by: Id,
    pub uses: u64,
    pub created_at: UTCDate,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_channel_id: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<UTCDate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_uses: Option<u64>,
}

/// A ban preventing a user from accessing a Space.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceBan {
    pub id: Id,
    pub space_id: Id,
    pub user_id: Id,
    pub banned_by: Id,
    pub created_at: UTCDate,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<UTCDate>,
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
