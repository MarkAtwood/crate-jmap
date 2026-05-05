//! RFC 9553 §2.3–2.4 contact channel types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── EmailAddress ──────────────────────────────────────────────────────────────

/// An email address for a contact (RFC 9553 §2.3.1).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailAddress {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── OnlineService ─────────────────────────────────────────────────────────────

/// An online service / messaging account for a contact (RFC 9553 §2.3.3).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnlineService {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── Phone ─────────────────────────────────────────────────────────────────────

/// A phone number for a contact (RFC 9553 §2.3.2).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Phone {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub features: Option<HashMap<String, bool>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── LanguagePref ──────────────────────────────────────────────────────────────

/// A preferred language for the contact (RFC 9553 §2.6.3).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguagePref {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,

    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── CalendarKind ──────────────────────────────────────────────────────────────

/// The kind of a calendar URI (RFC 9553 §2.3.5).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CalendarKind {
    Calendar,
    FreeBusy,
    Other(String),
}

impl_string_enum!(CalendarKind, "a calendar kind",
    "calendar" => Calendar,
    "freeBusy" => FreeBusy,
);

// ── Calendar ──────────────────────────────────────────────────────────────────

/// A calendar URI for a contact (RFC 9553 §2.3.5).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Calendar {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<CalendarKind>,

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

// ── SchedulingAddress ─────────────────────────────────────────────────────────

/// A scheduling (CalDAV) address for a contact (RFC 9553 §2.3.6).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulingAddress {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 9553 Figure 25 — email address round-trip.
    #[test]
    fn email_address_round_trip() {
        let json = r#"{
            "@type": "EmailAddress",
            "address": "jqpublic@xyz.example.com",
            "contexts": { "work": true }
        }"#;

        let email: EmailAddress = serde_json::from_str(json).expect("deserialize EmailAddress");

        assert_eq!(email.at_type.as_deref(), Some("EmailAddress"));
        assert_eq!(email.address.as_deref(), Some("jqpublic@xyz.example.com"));
        let ctx = email.contexts.as_ref().expect("contexts present");
        assert_eq!(ctx.get("work"), Some(&true));

        // Re-serialise and round-trip back.
        let re = serde_json::to_string(&email).expect("serialize EmailAddress");
        let email2: EmailAddress = serde_json::from_str(&re).expect("deserialize again");
        assert_eq!(email2.address.as_deref(), Some("jqpublic@xyz.example.com"));
    }

    /// EmailAddress extra vendor fields are preserved.
    #[test]
    fn email_address_extra_fields() {
        let json = r#"{ "address": "x@example.com", "x-vendor": "yes" }"#;
        let email: EmailAddress = serde_json::from_str(json).unwrap();
        assert_eq!(
            email.extra.get("x-vendor").and_then(|v| v.as_str()),
            Some("yes")
        );
    }

    /// CalendarKind wire values.
    #[test]
    fn calendar_kind_wire() {
        let k: CalendarKind = serde_json::from_str(r#""freeBusy""#).unwrap();
        assert_eq!(k, CalendarKind::FreeBusy);
        assert_eq!(serde_json::to_string(&k).unwrap(), r#""freeBusy""#);

        let k2: CalendarKind = serde_json::from_str(r#""calendar""#).unwrap();
        assert_eq!(k2, CalendarKind::Calendar);
    }

    /// Phone round-trip with features map.
    #[test]
    fn phone_round_trip() {
        let json = r#"{
            "@type": "Phone",
            "number": "+1-555-867-5309",
            "features": { "voice": true, "cell": true },
            "contexts": { "work": true }
        }"#;
        let phone: Phone = serde_json::from_str(json).unwrap();
        let feats = phone.features.as_ref().unwrap();
        assert_eq!(feats.get("voice"), Some(&true));
        let re = serde_json::to_string(&phone).unwrap();
        let phone2: Phone = serde_json::from_str(&re).unwrap();
        assert_eq!(phone2.number.as_deref(), Some("+1-555-867-5309"));
    }
}
