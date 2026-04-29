use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

/// A server-defined or space-scoped custom emoji (spec: CustomEmoji object).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomEmoji {
    pub id: Id,
    pub name: String,
    pub blob_id: Id,
    pub created_by: Id,
    pub created_at: UTCDate,
    /// If absent, the emoji is server-global; if present, scoped to that Space.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_id: Option<Id>,
}

impl CustomEmoji {
    /// Construct a [`CustomEmoji`] from its required fields.
    ///
    /// `space_id` defaults to `None` (server-global emoji).
    pub fn new(
        id: Id,
        name: impl Into<String>,
        blob_id: Id,
        created_by: Id,
        created_at: UTCDate,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            blob_id,
            created_by,
            created_at,
            space_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Oracle: JSON without space_id — server-global emoji; space_id must be None.
    #[test]
    fn emoji_server_global() {
        let json = r#"{
            "id": "e1",
            "name": "partyblob",
            "blobId": "b1",
            "createdBy": "u1",
            "createdAt": "2026-01-15T10:00:00Z"
        }"#;
        let e: CustomEmoji = serde_json::from_str(json).expect("deserialize CustomEmoji");
        assert_eq!(e.id.as_ref(), "e1");
        assert_eq!(e.name, "partyblob");
        assert!(e.space_id.is_none());
    }

    // Oracle: full JSON with space_id round-trips without data loss.
    #[test]
    fn emoji_space_scoped() {
        let json = r#"{
            "id": "e2",
            "name": "teamrocket",
            "blobId": "b2",
            "createdBy": "u2",
            "createdAt": "2026-02-01T00:00:00Z",
            "spaceId": "s1"
        }"#;
        let e: CustomEmoji = serde_json::from_str(json).expect("deserialize");
        assert_eq!(e.space_id.as_ref().map(|id| id.as_ref()), Some("s1"));
        let serialized = serde_json::to_string(&e).expect("serialize");
        let e2: CustomEmoji = serde_json::from_str(&serialized).expect("re-deserialize");
        assert_eq!(e, e2);
    }
}
