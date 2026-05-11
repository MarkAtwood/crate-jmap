//! CustomEmoji object for server-global and space-scoped emoji.

use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

/// A server-defined or space-scoped custom emoji (spec: CustomEmoji object).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomEmoji {
    /// The `id` property (draft-atwood-jmap-chat-00 §4.17).
    pub id: Id,
    /// The `name` property (draft-atwood-jmap-chat-00 §4.17).
    pub name: String,
    /// The `blobId` property (draft-atwood-jmap-chat-00 §4.17).
    pub blob_id: Id,
    /// The `createdBy` property (draft-atwood-jmap-chat-00 §4.17).
    pub created_by: Id,
    /// The `createdAt` property (draft-atwood-jmap-chat-00 §4.17).
    pub created_at: UTCDate,
    /// The `spaceId` property (draft-atwood-jmap-chat-00 §4.17).
    ///
    /// If absent, the emoji is server-global; if present, scoped to that Space.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_id: Option<Id>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
            extra: serde_json::Map::new(),
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

    // ── Extras-preservation policy tests (JMAP-lbdy.3) ───────────────────

    /// `CustomEmoji.extra` captures vendor fields and preserves them.
    #[test]
    fn custom_emoji_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "id": "e1",
            "name": "partyblob",
            "blobId": "b1",
            "createdBy": "u1",
            "createdAt": "2026-01-15T10:00:00Z",
            "acmeCorpReviewedBy": "moderator-7"
        });
        let e: CustomEmoji = serde_json::from_value(raw).unwrap();
        assert_eq!(
            e.extra.get("acmeCorpReviewedBy").and_then(|v| v.as_str()),
            Some("moderator-7")
        );
        let back = serde_json::to_value(&e).unwrap();
        assert_eq!(back["acmeCorpReviewedBy"], "moderator-7");
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
