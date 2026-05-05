//! RFC 9553 §2.2 name-related types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── NameComponentKind ─────────────────────────────────────────────────────────

/// The kind of a single name component (RFC 9553 §2.2.1).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NameComponentKind {
    Given,
    Surname,
    Suffix,
    Prefix,
    Credential,
    Generation,
    Separator,
    Given2,
    Surname2,
    Title,
    Name,
    Other(String),
}

impl_string_enum!(NameComponentKind, "a name component kind",
    "given"      => Given,
    "surname"    => Surname,
    "suffix"     => Suffix,
    "prefix"     => Prefix,
    "credential" => Credential,
    "generation" => Generation,
    "separator"  => Separator,
    "given2"     => Given2,
    "surname2"   => Surname2,
    "title"      => Title,
    "name"       => Name,
);

// ── NameComponent ─────────────────────────────────────────────────────────────

/// A single component of a structured name (RFC 9553 §2.2.1).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NameComponent {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<NameComponentKind>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub phonetic: Option<String>,

    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── PhoneticSystem ────────────────────────────────────────────────────────────

/// Phonetic system used for pronunciation guidance (RFC 9553 §2.2.1, §2.5.2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PhoneticSystem {
    Ipa,
    Jyut,
    Piny,
    Other(String),
}

impl_string_enum!(PhoneticSystem, "a phonetic system",
    "ipa"  => Ipa,
    "jyut" => Jyut,
    "piny" => Piny,
);

// ── Name ─────────────────────────────────────────────────────────────────────

/// A structured name (RFC 9553 §2.2.1).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Name {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub components: Option<Vec<NameComponent>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_ordered: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_separator: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub full: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_as: Option<HashMap<String, String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub phonetic_script: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub phonetic_system: Option<PhoneticSystem>,

    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── GrammaticalGender ─────────────────────────────────────────────────────────

/// Grammatical gender for a contact (RFC 9553 §2.2.5).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GrammaticalGender {
    Animate,
    Common,
    Feminine,
    Inanimate,
    Masculine,
    Neuter,
    Other(String),
}

impl_string_enum!(GrammaticalGender, "a grammatical gender",
    "animate"   => Animate,
    "common"    => Common,
    "feminine"  => Feminine,
    "inanimate" => Inanimate,
    "masculine" => Masculine,
    "neuter"    => Neuter,
);

// ── Pronouns ──────────────────────────────────────────────────────────────────

/// Pronouns to use for a contact in a given context (RFC 9553 §2.2.5).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pronouns {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pronouns: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,

    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── SpeakToAs ─────────────────────────────────────────────────────────────────

/// How to address the contact (RFC 9553 §2.2.5).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeakToAs {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub grammatical_gender: Option<GrammaticalGender>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pronouns: Option<HashMap<String, Pronouns>>,

    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── Nickname ──────────────────────────────────────────────────────────────────

/// A nickname for a contact (RFC 9553 §2.2.4).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Nickname {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,

    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── OrgUnit ───────────────────────────────────────────────────────────────────

/// An organizational unit within an organization (RFC 9553 §2.2.3).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgUnit {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_as: Option<String>,

    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── Organization ──────────────────────────────────────────────────────────────

/// An organization the contact is associated with (RFC 9553 §2.2.3).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Organization {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub units: Option<Vec<OrgUnit>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_as: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── TitleKind ─────────────────────────────────────────────────────────────────

/// The kind of a title entry (RFC 9553 §2.2.6).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TitleKind {
    Title,
    Role,
    Other(String),
}

impl_string_enum!(TitleKind, "a title kind",
    "title" => Title,
    "role"  => Role,
);

// ── Title ─────────────────────────────────────────────────────────────────────

/// A title or role held by the contact (RFC 9553 §2.2.6).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Title {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<TitleKind>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization_id: Option<jmap_types::Id>,

    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 9553 Figure 16 — structured name round-trip.
    #[test]
    fn name_round_trip() {
        let json = r#"{
            "@type": "Name",
            "components": [
                { "kind": "given",   "value": "Vincent" },
                { "kind": "surname", "value": "van Gogh" }
            ],
            "isOrdered": true
        }"#;

        let name: Name = serde_json::from_str(json).expect("deserialize Name");

        assert_eq!(name.at_type.as_deref(), Some("Name"));
        assert_eq!(name.is_ordered, Some(true));

        let comps = name.components.as_ref().expect("components present");
        assert_eq!(comps.len(), 2);
        assert_eq!(comps[0].kind, Some(NameComponentKind::Given));
        assert_eq!(comps[0].value.as_deref(), Some("Vincent"));
        assert_eq!(comps[1].kind, Some(NameComponentKind::Surname));
        assert_eq!(comps[1].value.as_deref(), Some("van Gogh"));

        // Re-serialise and round-trip back.
        let re = serde_json::to_string(&name).expect("serialize Name");
        let name2: Name = serde_json::from_str(&re).expect("deserialize Name again");
        assert_eq!(name2.is_ordered, Some(true));
        let comps2 = name2.components.as_ref().expect("components present");
        assert_eq!(comps2[0].value.as_deref(), Some("Vincent"));
    }

    /// Unknown fields are preserved in `extra`.
    #[test]
    fn name_component_extra_fields() {
        let json = r#"{ "kind": "given", "value": "Ada", "x-vendor-foo": "bar" }"#;
        let comp: NameComponent = serde_json::from_str(json).expect("deserialize NameComponent");
        assert_eq!(
            comp.extra.get("x-vendor-foo").and_then(|v| v.as_str()),
            Some("bar")
        );
    }

    /// NameComponentKind unknown value maps to Other.
    #[test]
    fn name_component_kind_other() {
        let json = r#""x-custom-kind""#;
        let kind: NameComponentKind = serde_json::from_str(json).expect("deserialize kind");
        assert_eq!(kind, NameComponentKind::Other("x-custom-kind".to_owned()));
        let re = serde_json::to_string(&kind).expect("serialize kind");
        assert_eq!(re, r#""x-custom-kind""#);
    }

    /// GrammaticalGender round-trip.
    #[test]
    fn grammatical_gender_round_trip() {
        for (wire, variant) in [
            ("\"animate\"", GrammaticalGender::Animate),
            ("\"feminine\"", GrammaticalGender::Feminine),
            ("\"masculine\"", GrammaticalGender::Masculine),
        ] {
            let g: GrammaticalGender = serde_json::from_str(wire).unwrap();
            assert_eq!(g, variant);
            assert_eq!(serde_json::to_string(&g).unwrap(), wire);
        }
    }

    /// TitleKind round-trip.
    #[test]
    fn title_kind_round_trip() {
        let json = r#"{"name":"CEO","kind":"role"}"#;
        let t: Title = serde_json::from_str(json).unwrap();
        assert_eq!(t.kind, Some(TitleKind::Role));
        let re = serde_json::to_string(&t).unwrap();
        let t2: Title = serde_json::from_str(&re).unwrap();
        assert_eq!(t2.name.as_deref(), Some("CEO"));
    }
}
