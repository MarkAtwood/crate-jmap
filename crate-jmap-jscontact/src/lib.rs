//! Typed Rust structs for RFC 9553 JSContact sub-objects.
//!
//! # Design
//!
//! [`ContactCard`] in `jmap-contacts-types` stores all JSContact sub-objects
//! as `serde_json::Value` for round-trip fidelity (vendor extension fields
//! are preserved). This crate provides typed structs that callers use via
//! explicit conversion:
//!
//! ```rust,ignore
//! use jmap_jscontact::Name;
//!
//! let name: Option<Name> = card.name
//!     .as_ref()
//!     .and_then(|v| serde_json::from_value(v.clone()).ok());
//! ```
//!
//! All structs are `#[non_exhaustive]` and include an `extra` field
//! (`#[serde(flatten)] pub extra: std::collections::HashMap<String, serde_json::Value>`)
//! that captures unknown vendor-specific properties without data loss.
//!
//! Spec: [RFC 9553](https://www.rfc-editor.org/rfc/rfc9553)

#![forbid(unsafe_code)]

#[macro_use]
mod string_enum;

pub mod address;
pub mod anniversary;
pub mod contact;
pub mod name;
pub mod note;
pub mod personal;
pub mod resource;

pub use address::{Address, AddressComponent, AddressComponentKind};
pub use anniversary::{Anniversary, AnniversaryDate, AnniversaryKind, PartialDate, Timestamp};
pub use contact::{
    Calendar, CalendarKind, EmailAddress, LanguagePref, OnlineService, Phone, SchedulingAddress,
};
pub use name::{
    GrammaticalGender, Name, NameComponent, NameComponentKind, Nickname, OrgUnit, Organization,
    PhoneticSystem, Pronouns, SpeakToAs, Title, TitleKind,
};
pub use note::{Author, Note};
pub use personal::{PersonalInfo, PersonalInfoKind, PersonalInfoLevel, Relation};
pub use resource::{CryptoKey, Directory, DirectoryKind, Link, LinkKind, Media, MediaKind};

// ---------------------------------------------------------------------------
// Typed accessor helpers
//
// Each function accepts a &serde_json::Value (e.g. from ContactCard.name) and
// returns a typed sub-object if deserialization succeeds.  Returns None if the
// value is absent, null, or uses an unknown schema (e.g. a future version of
// the spec).  Unknown vendor extension fields are preserved in `extra`.
//
// Usage:
//   let name: Option<Name> = jmap_jscontact::parse_name(card.name.as_ref()?);
// ---------------------------------------------------------------------------

/// Parse a JSContact `Name` object from a `serde_json::Value`.
///
/// Returns `None` if the value is `None`, not a JSON object, or cannot be
/// deserialized into [`Name`].
pub fn parse_name(v: Option<&serde_json::Value>) -> Option<Name> {
    v.and_then(|v| serde_json::from_value(v.clone()).ok())
}

/// Parse an `Id[Nickname]` map from a `serde_json::Value`.
pub fn parse_nicknames(
    v: Option<&serde_json::Value>,
) -> std::collections::HashMap<String, Nickname> {
    v.and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Parse an `Id[Organization]` map from a `serde_json::Value`.
pub fn parse_organizations(
    v: Option<&serde_json::Value>,
) -> std::collections::HashMap<String, Organization> {
    v.and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Parse a `SpeakToAs` object from a `serde_json::Value`.
pub fn parse_speak_to_as(v: Option<&serde_json::Value>) -> Option<SpeakToAs> {
    v.and_then(|v| serde_json::from_value(v.clone()).ok())
}

/// Parse an `Id[Title]` map from a `serde_json::Value`.
pub fn parse_titles(v: Option<&serde_json::Value>) -> std::collections::HashMap<String, Title> {
    v.and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Parse an `Id[EmailAddress]` map from a `serde_json::Value`.
pub fn parse_emails(
    v: Option<&serde_json::Value>,
) -> std::collections::HashMap<String, EmailAddress> {
    v.and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Parse an `Id[OnlineService]` map from a `serde_json::Value`.
pub fn parse_online_services(
    v: Option<&serde_json::Value>,
) -> std::collections::HashMap<String, OnlineService> {
    v.and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Parse an `Id[Phone]` map from a `serde_json::Value`.
pub fn parse_phones(v: Option<&serde_json::Value>) -> std::collections::HashMap<String, Phone> {
    v.and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Parse an `Id[LanguagePref]` map from a `serde_json::Value`.
pub fn parse_preferred_languages(
    v: Option<&serde_json::Value>,
) -> std::collections::HashMap<String, LanguagePref> {
    v.and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Parse an `Id[Calendar]` map from a `serde_json::Value`.
pub fn parse_calendars(
    v: Option<&serde_json::Value>,
) -> std::collections::HashMap<String, Calendar> {
    v.and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Parse an `Id[SchedulingAddress]` map from a `serde_json::Value`.
pub fn parse_scheduling_addresses(
    v: Option<&serde_json::Value>,
) -> std::collections::HashMap<String, SchedulingAddress> {
    v.and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Parse an `Id[Address]` map from a `serde_json::Value`.
pub fn parse_addresses(
    v: Option<&serde_json::Value>,
) -> std::collections::HashMap<String, Address> {
    v.and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Parse an `Id[CryptoKey]` map from a `serde_json::Value`.
pub fn parse_crypto_keys(
    v: Option<&serde_json::Value>,
) -> std::collections::HashMap<String, CryptoKey> {
    v.and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Parse an `Id[Directory]` map from a `serde_json::Value`.
pub fn parse_directories(
    v: Option<&serde_json::Value>,
) -> std::collections::HashMap<String, Directory> {
    v.and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Parse an `Id[Link]` map from a `serde_json::Value`.
pub fn parse_links(v: Option<&serde_json::Value>) -> std::collections::HashMap<String, Link> {
    v.and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Parse an `Id[Media]` map from a `serde_json::Value`.
pub fn parse_media(v: Option<&serde_json::Value>) -> std::collections::HashMap<String, Media> {
    v.and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Parse an `Id[Anniversary]` map from a `serde_json::Value`.
pub fn parse_anniversaries(
    v: Option<&serde_json::Value>,
) -> std::collections::HashMap<String, Anniversary> {
    v.and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Parse an `Id[Note]` map from a `serde_json::Value`.
pub fn parse_notes(v: Option<&serde_json::Value>) -> std::collections::HashMap<String, Note> {
    v.and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Parse an `Id[PersonalInfo]` map from a `serde_json::Value`.
pub fn parse_personal_info(
    v: Option<&serde_json::Value>,
) -> std::collections::HashMap<String, PersonalInfo> {
    v.and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Parse a `String[Relation]` map from a `serde_json::Value`.
pub fn parse_related_to(
    v: Option<&serde_json::Value>,
) -> std::collections::HashMap<String, Relation> {
    v.and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod lib_tests {
    use super::*;
    use serde_json::json;

    /// Oracle: parse_name returns None for None input.
    #[test]
    fn parse_name_none_input() {
        assert!(parse_name(None).is_none());
    }

    /// Oracle: parse_name returns None for non-object JSON (e.g. null).
    #[test]
    fn parse_name_null_input() {
        let v = json!(null);
        assert!(parse_name(Some(&v)).is_none());
    }

    /// Oracle: parse_name round-trips a Name through Value (the core use case).
    /// This simulates ContactCard.name → parse_name() → Name struct.
    /// Spec oracle: RFC 9553 Figure 16 — Vincent van Gogh name example.
    #[test]
    fn parse_name_round_trip_via_value() {
        let raw = json!({
            "@type": "Name",
            "components": [
                { "kind": "given", "value": "Vincent" },
                { "kind": "surname", "value": "van Gogh" }
            ],
            "isOrdered": true
        });

        let name = parse_name(Some(&raw)).expect("must deserialize Name from Value");
        assert_eq!(
            name.components.as_ref().map(|c| c.len()),
            Some(2),
            "must have 2 components"
        );
        let comps = name.components.as_ref().unwrap();
        assert_eq!(comps[0].kind, Some(NameComponentKind::Given));
        assert_eq!(comps[0].value.as_deref(), Some("Vincent"));
        assert_eq!(comps[1].kind, Some(NameComponentKind::Surname));
    }

    /// Oracle: parse_emails returns empty map for None input.
    #[test]
    fn parse_emails_none_returns_empty_map() {
        let result = parse_emails(None);
        assert!(result.is_empty());
    }

    /// Oracle: parse_emails round-trips Id[EmailAddress] through Value.
    /// Spec oracle: RFC 9553 Figure 25.
    #[test]
    fn parse_emails_round_trip_via_value() {
        let raw = json!({
            "e1": {
                "@type": "EmailAddress",
                "address": "jqpublic@xyz.example.com",
                "contexts": { "work": true }
            },
            "e2": {
                "address": "jane_doe@example.com",
                "pref": 1
            }
        });

        let emails = parse_emails(Some(&raw));
        assert_eq!(emails.len(), 2);
        assert_eq!(
            emails["e1"].address.as_deref(),
            Some("jqpublic@xyz.example.com")
        );
        assert_eq!(emails["e2"].pref, Some(1));
    }

    /// Oracle: extension fields in vendor-extended sub-objects are preserved
    /// through the parse_* helpers (round-trip fidelity via extra HashMap).
    #[test]
    fn parse_name_preserves_vendor_extension() {
        let raw = json!({
            "@type": "Name",
            "full": "Alice Smith",
            "x-custom-vendor-field": "custom-value"
        });

        let name = parse_name(Some(&raw)).expect("must parse");
        assert_eq!(name.full.as_deref(), Some("Alice Smith"));
        assert!(
            name.extra.contains_key("x-custom-vendor-field"),
            "vendor extension field must be preserved in extra"
        );
    }

    /// Oracle: parse_addresses round-trips Id[Address] through Value.
    #[test]
    fn parse_addresses_round_trip() {
        let raw = json!({
            "addr1": {
                "@type": "Address",
                "full": "54321 Oak St, Anytown, US",
                "countryCode": "US"
            }
        });

        let addrs = parse_addresses(Some(&raw));
        assert_eq!(addrs.len(), 1);
        assert_eq!(addrs["addr1"].country_code.as_deref(), Some("US"));
    }

    /// Oracle: parse_related_to round-trips String[Relation] through Value.
    /// Spec oracle: RFC 9553 §2.1.8.
    #[test]
    fn parse_related_to_round_trip() {
        let raw = json!({
            "uid-of-sibling": {
                "@type": "Relation",
                "relation": { "sibling": true }
            }
        });

        let related = parse_related_to(Some(&raw));
        assert_eq!(related.len(), 1);
        let rel = &related["uid-of-sibling"];
        assert_eq!(rel.relation.get("sibling"), Some(&true));
    }
}
