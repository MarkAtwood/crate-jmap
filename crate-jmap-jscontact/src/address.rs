//! RFC 9553 §2.5 address types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── AddressComponentKind ──────────────────────────────────────────────────────

/// The kind of a single address component (RFC 9553 §2.5.1).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AddressComponentKind {
    Room,
    Apartment,
    Floor,
    Building,
    Number,
    Direction,
    Landmark,
    Block,
    SubDistrict,
    District,
    Locality,
    Region,
    Postcode,
    Country,
    Separator,
    Name,
    PostOfficeBox,
    Other(String),
}

impl_string_enum!(AddressComponentKind, "an address component kind",
    "room"          => Room,
    "apartment"     => Apartment,
    "floor"         => Floor,
    "building"      => Building,
    "number"        => Number,
    "direction"     => Direction,
    "landmark"      => Landmark,
    "block"         => Block,
    "subDistrict"   => SubDistrict,
    "district"      => District,
    "locality"      => Locality,
    "region"        => Region,
    "postcode"      => Postcode,
    "country"       => Country,
    "separator"     => Separator,
    "name"          => Name,
    "postOfficeBox" => PostOfficeBox,
);

// ── AddressComponent ──────────────────────────────────────────────────────────

/// A single component of a structured postal address (RFC 9553 §2.5.1).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressComponent {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<AddressComponentKind>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub phonetic: Option<String>,

    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── Address ───────────────────────────────────────────────────────────────────

/// A postal address for a contact (RFC 9553 §2.5.1).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Address {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub components: Option<Vec<AddressComponent>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_ordered: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub country_code: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordinates: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub full: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_separator: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub phonetic_script: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub phonetic_system: Option<crate::name::PhoneticSystem>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,

    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Basic address round-trip.
    #[test]
    fn address_round_trip() {
        let json = r#"{
            "@type": "Address",
            "components": [
                { "kind": "number",   "value": "54321" },
                { "kind": "locality", "value": "Reston" },
                { "kind": "region",   "value": "VA" },
                { "kind": "country",  "value": "US" }
            ],
            "isOrdered": true,
            "countryCode": "US"
        }"#;

        let addr: Address = serde_json::from_str(json).expect("deserialize Address");
        assert_eq!(addr.at_type.as_deref(), Some("Address"));
        assert_eq!(addr.country_code.as_deref(), Some("US"));
        let comps = addr.components.as_ref().unwrap();
        assert_eq!(comps.len(), 4);
        assert_eq!(comps[1].kind, Some(AddressComponentKind::Locality));

        let re = serde_json::to_string(&addr).unwrap();
        let addr2: Address = serde_json::from_str(&re).unwrap();
        assert_eq!(addr2.country_code.as_deref(), Some("US"));
    }

    /// AddressComponentKind unknown value maps to Other.
    #[test]
    fn address_component_kind_other() {
        let kind: AddressComponentKind = serde_json::from_str(r#""x-wing""#).unwrap();
        assert_eq!(kind, AddressComponentKind::Other("x-wing".to_owned()));
    }
}
