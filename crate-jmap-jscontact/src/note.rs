//! RFC 9553 §2.8 note types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Author ────────────────────────────────────────────────────────────────────

/// The author of a note (RFC 9553 §2.8.1).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Author {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── Note ──────────────────────────────────────────────────────────────────────

/// A free-form note about a contact (RFC 9553 §2.8.1).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<Author>,

    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_round_trip() {
        let json = r#"{
            "@type": "Note",
            "note": "Met at conference",
            "created": "2024-01-15T10:00:00Z",
            "author": { "@type": "Author", "name": "Alice" }
        }"#;

        let note: Note = serde_json::from_str(json).expect("deserialize Note");
        assert_eq!(note.note.as_deref(), Some("Met at conference"));
        assert_eq!(
            note.author.as_ref().and_then(|a| a.name.as_deref()),
            Some("Alice")
        );

        let re = serde_json::to_string(&note).unwrap();
        let note2: Note = serde_json::from_str(&re).unwrap();
        assert_eq!(note2.note.as_deref(), Some("Met at conference"));
    }
}
