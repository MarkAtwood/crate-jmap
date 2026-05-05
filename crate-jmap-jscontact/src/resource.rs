//! RFC 9553 §2.6 resource types (links, media, crypto keys, directories).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── LinkKind ──────────────────────────────────────────────────────────────────

/// The kind of a link resource (RFC 9553 §2.6.1).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LinkKind {
    Contact,
    Other(String),
}

impl_string_enum!(LinkKind, "a link kind",
    "contact" => Contact,
);

// ── Link ──────────────────────────────────────────────────────────────────────

/// A URI link resource for a contact (RFC 9553 §2.6.1).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<LinkKind>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── MediaKind ─────────────────────────────────────────────────────────────────

/// The kind of a media resource (RFC 9553 §2.6.4).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MediaKind {
    Photo,
    Sound,
    Logo,
    Other(String),
}

impl_string_enum!(MediaKind, "a media kind",
    "photo" => Photo,
    "sound" => Sound,
    "logo"  => Logo,
);

// ── Media ─────────────────────────────────────────────────────────────────────

/// A media resource (photo, logo, sound clip) for a contact (RFC 9553 §2.6.4).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Media {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<MediaKind>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── CryptoKey ─────────────────────────────────────────────────────────────────

/// A cryptographic key resource for a contact (RFC 9553 §2.6.5).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CryptoKey {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── DirectoryKind ─────────────────────────────────────────────────────────────

/// The kind of a directory resource (RFC 9553 §2.6.2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DirectoryKind {
    Directory,
    Entry,
    Other(String),
}

impl_string_enum!(DirectoryKind, "a directory kind",
    "directory" => Directory,
    "entry"     => Entry,
);

// ── Directory ─────────────────────────────────────────────────────────────────

/// A directory resource (LDAP, vCard source, etc.) for a contact (RFC 9553 §2.6.2).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Directory {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<DirectoryKind>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,

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
    fn link_round_trip() {
        let json = r#"{
            "@type": "Link",
            "kind": "contact",
            "uri": "https://example.com/vcard/jdoe.vcf"
        }"#;
        let link: Link = serde_json::from_str(json).unwrap();
        assert_eq!(link.kind, Some(LinkKind::Contact));
        assert_eq!(
            link.uri.as_deref(),
            Some("https://example.com/vcard/jdoe.vcf")
        );
        let re = serde_json::to_string(&link).unwrap();
        let link2: Link = serde_json::from_str(&re).unwrap();
        assert_eq!(
            link2.uri.as_deref(),
            Some("https://example.com/vcard/jdoe.vcf")
        );
    }

    #[test]
    fn media_kind_round_trip() {
        for (wire, variant) in [
            ("\"photo\"", MediaKind::Photo),
            ("\"sound\"", MediaKind::Sound),
            ("\"logo\"", MediaKind::Logo),
        ] {
            let k: MediaKind = serde_json::from_str(wire).unwrap();
            assert_eq!(k, variant);
            assert_eq!(serde_json::to_string(&k).unwrap(), wire);
        }
    }

    #[test]
    fn directory_kind_round_trip() {
        let k: DirectoryKind = serde_json::from_str(r#""entry""#).unwrap();
        assert_eq!(k, DirectoryKind::Entry);
    }
}
