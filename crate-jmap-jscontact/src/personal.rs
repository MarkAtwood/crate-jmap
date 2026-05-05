//! RFC 9553 §2.9 personal information types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── PersonalInfoKind ──────────────────────────────────────────────────────────

/// The kind of personal information entry (RFC 9553 §2.9.1).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PersonalInfoKind {
    Expertise,
    Hobby,
    Interest,
    Other(String),
}

impl_string_enum!(PersonalInfoKind, "a personal info kind",
    "expertise" => Expertise,
    "hobby"     => Hobby,
    "interest"  => Interest,
);

// ── PersonalInfoLevel ─────────────────────────────────────────────────────────

/// The level of expertise or interest (RFC 9553 §2.9.1).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PersonalInfoLevel {
    High,
    Medium,
    Low,
    Other(String),
}

impl_string_enum!(PersonalInfoLevel, "a personal info level",
    "high"   => High,
    "medium" => Medium,
    "low"    => Low,
);

// ── PersonalInfo ──────────────────────────────────────────────────────────────

/// A personal interest, hobby, or area of expertise (RFC 9553 §2.9.1).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalInfo {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<PersonalInfoKind>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<PersonalInfoLevel>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_as: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn personal_info_round_trip() {
        let json = r#"{
            "@type": "PersonalInfo",
            "kind": "hobby",
            "value": "Cycling",
            "level": "high"
        }"#;

        let pi: PersonalInfo = serde_json::from_str(json).expect("deserialize PersonalInfo");
        assert_eq!(pi.kind, Some(PersonalInfoKind::Hobby));
        assert_eq!(pi.value.as_deref(), Some("Cycling"));
        assert_eq!(pi.level, Some(PersonalInfoLevel::High));

        let re = serde_json::to_string(&pi).unwrap();
        let pi2: PersonalInfo = serde_json::from_str(&re).unwrap();
        assert_eq!(pi2.kind, Some(PersonalInfoKind::Hobby));
    }

    #[test]
    fn personal_info_kind_other() {
        let k: PersonalInfoKind = serde_json::from_str(r#""x-passion""#).unwrap();
        assert_eq!(k, PersonalInfoKind::Other("x-passion".to_owned()));
    }
}

/// A relationship between this Card and another entity (RFC 9553 §2.1.8).
///
/// Used as the value type in `relatedTo: String[Relation]` on `ContactCard`,
/// where the map key is the `uid` of the related Card.
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Relation {
    /// Object type discriminator; when set MUST be `"Relation"`.
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// Set of relation type strings → `true`.  Empty map means the
    /// relationship type is undefined (RFC 9553 §2.1.8).
    /// Common values: `acquaintance`, `agent`, `child`, `co-resident`,
    /// `co-worker`, `colleague`, `contact`, `crush`, `date`, `emergency`,
    /// `friend`, `kin`, `me`, `met`, `muse`, `neighbor`, `parent`,
    /// `sibling`, `spouse`, `sweetheart`.
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub relation: std::collections::HashMap<String, bool>,

    /// Unknown vendor-specific extension fields (preserved for round-trip fidelity).
    #[serde(flatten, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}
