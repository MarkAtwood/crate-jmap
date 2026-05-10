//! RFC 9670 §2 Principal object and related types.
//!
//! Provides [`Principal`], [`PrincipalType`], and [`PrincipalFilterCondition`].
//! A Principal represents an individual, a group, a location, a resource, or
//! another entity in a collaborative JMAP environment.

use std::collections::HashMap;

use jmap_types::{impl_string_enum, Id};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The type of a Principal (RFC 9670 §2).
///
/// Implementations MUST handle unknown values gracefully.  The `Other(String)`
/// variant retains the original wire string for round-tripping.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PrincipalType {
    /// A single person.
    Individual,
    /// A group of other Principals.
    Group,
    /// A resource, e.g., a projector.
    Resource,
    /// A location, e.g., a meeting room.
    Location,
    /// Any other undefined Principal type defined by RFC 9670 §2.
    ///
    /// Note: the RFC defines `"other"` as a known value meaning "some other
    /// undefined Principal".  Unknown future values also round-trip here.
    Other(String),
}

impl_string_enum!(
    PrincipalType,
    "a JMAP Principal type string",
    "individual" => Individual,
    "group"      => Group,
    "resource"   => Resource,
    "location"   => Location,
);

/// A JMAP Principal object (RFC 9670 §2).
///
/// Represents an individual, a group, a location, a resource, or another
/// entity in a collaborative environment.  Sharing in JMAP is configured by
/// assigning rights to data within an Account to other Principals.
///
/// ## Nullable vs absent fields
///
/// The RFC marks `description`, `email`, `timeZone`, and `accounts` as
/// required but nullable.  These fields MUST serialize as `null` when their
/// Rust value is `None` — they must NOT be absent from the wire JSON.
/// Accordingly, none of these fields use `#[serde(skip_serializing_if)]`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Principal {
    /// Server-assigned immutable identifier.
    pub id: Id,

    /// The type of this Principal.
    #[serde(rename = "type")]
    pub principal_type: PrincipalType,

    /// Human-readable display name, e.g., "Jane Doe" or "Room 4B".
    pub name: String,

    /// A longer description, or `null` if none is available.
    ///
    /// Serializes as `null` when `None` (required-but-nullable per RFC 9670 §2).
    pub description: Option<String>,

    /// Email address for this Principal (addr-spec syntax), or `null`.
    ///
    /// Serializes as `null` when `None` (required-but-nullable per RFC 9670 §2).
    pub email: Option<String>,

    /// IANA time zone name for this Principal, or `null` if unknown.
    ///
    /// Serializes as `null` when `None` (required-but-nullable per RFC 9670 §2).
    #[serde(rename = "timeZone")]
    pub time_zone: Option<String>,

    /// Map of JMAP capability URIs to domain-specific capability objects (server-set).
    ///
    /// Uses `serde_json::Value` because each capability's object schema is defined
    /// by its own specification; this crate cannot enumerate all possible shapes.
    pub capabilities: HashMap<String, Value>,

    /// Map of Account id to Account object for each Account containing data for
    /// this Principal that the user has access to, or `null` if none.
    ///
    /// Uses `serde_json::Value` for the Account objects — see crate PLAN.md for
    /// the rationale.
    ///
    /// Serializes as `null` when `None` (required-but-nullable per RFC 9670 §2).
    pub accounts: Option<HashMap<Id, Value>>,
}

/// Filter condition for `Principal/query` (RFC 9670 §2.4.1).
///
/// All fields are optional; a condition with no fields set matches every Principal.
/// When multiple fields are set, all conditions must match (logical AND).
///
/// # Excluded from extras preservation
///
/// This type is **out of scope** for the workspace extras-preservation
/// policy: it carries no flatten-extras `extra` field. Filter clauses the
/// server does not understand are a query-correctness hazard — silently
/// preserving an unrecognised clause and round-tripping it back to the
/// client can return the wrong set of records with no error signal.
///
/// ## What to do instead
///
/// **IETF-track path.** Vendors who need both capability-level declaration
/// and filterability for custom fields should use
/// `draft-ietf-jmap-metadata` (capability URI
/// `urn:ietf:params:jmap:metadata`), which defines a filterable
/// `Metadata` / `Annotation` companion object. Workspace implementation
/// tracker: bd JMAP-06zp.
///
/// **Pre-IETF escape.** Vendors who cannot wait for the metadata draft can
/// either escape the filter tree to `serde_json::Value` or fork the
/// `FilterCondition` type. See `crate-jmap-calendars-types/PLAN.md` for
/// the hybrid sloppy-value pattern.
///
/// Cross-reference: bd JMAP-lbdy "Decision: filter algebra excluded".
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalFilterCondition {
    /// Match Principals that own at least one of these Account ids.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_ids: Option<Vec<String>>,

    /// The `email` property of the Principal must contain this string (substring).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// The `name` property of the Principal must contain this string (substring).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The `name`, `email`, or `description` of the Principal must contain this
    /// string (substring).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// The `type` of the Principal must exactly match this value.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<PrincipalType>,

    /// The `timeZone` of the Principal must exactly match this value.
    #[serde(rename = "timeZone", skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Oracle: RFC 9670 §4 example — hand-written from the spec.
    /// The spec example at §4.1 shows a Principal response with a single object.
    fn rfc_principal_json() -> serde_json::Value {
        json!({
            "id": "P2342fnddd20",
            "type": "individual",
            "name": "Joe Bloggs",
            "description": null,
            "email": "joe@example.com",
            "timeZone": "America/New_York",
            "capabilities": {},
            "accounts": null
        })
    }

    #[test]
    fn deserialize_rfc_principal() {
        let json = rfc_principal_json();
        let p: Principal = serde_json::from_value(json).expect("deserialize Principal");

        assert_eq!(p.id, "P2342fnddd20");
        assert_eq!(p.principal_type, PrincipalType::Individual);
        assert_eq!(p.name, "Joe Bloggs");
        assert_eq!(p.description, None);
        assert_eq!(p.email.as_deref(), Some("joe@example.com"));
        assert_eq!(p.time_zone.as_deref(), Some("America/New_York"));
        assert!(p.capabilities.is_empty());
        assert!(p.accounts.is_none());
    }

    #[test]
    fn principal_roundtrip() {
        let json = rfc_principal_json();
        let p = Principal::deserialize(&json).expect("deserialize");
        let serialized = serde_json::to_value(&p).expect("serialize");
        let p2: Principal = serde_json::from_value(serialized).expect("deserialize again");
        assert_eq!(p, p2);
    }

    /// Nullable fields must serialize as `null`, not be absent.
    #[test]
    fn nullable_fields_serialize_as_null() {
        let json = rfc_principal_json();
        let p: Principal = serde_json::from_value(json).expect("deserialize");
        let serialized = serde_json::to_value(&p).expect("serialize");

        // description is null in the oracle — must appear in output as null
        assert_eq!(serialized["description"], serde_json::Value::Null);
        // accounts is null — must appear as null
        assert_eq!(serialized["accounts"], serde_json::Value::Null);
    }

    /// Nullable fields: `null` in JSON → `None` in Rust → `null` in re-serialized JSON.
    #[test]
    fn null_deserializes_to_none_and_back() {
        let json = json!({
            "id": "abc1",
            "type": "group",
            "name": "Team Alpha",
            "description": null,
            "email": null,
            "timeZone": null,
            "capabilities": {},
            "accounts": null
        });
        let p: Principal = serde_json::from_value(json).expect("deserialize");
        assert!(p.description.is_none());
        assert!(p.email.is_none());
        assert!(p.time_zone.is_none());
        assert!(p.accounts.is_none());

        let reserialized = serde_json::to_value(&p).expect("serialize");
        assert_eq!(reserialized["description"], serde_json::Value::Null);
        assert_eq!(reserialized["email"], serde_json::Value::Null);
        assert_eq!(reserialized["timeZone"], serde_json::Value::Null);
        assert_eq!(reserialized["accounts"], serde_json::Value::Null);
    }

    #[test]
    fn principal_with_accounts() {
        let json = json!({
            "id": "P1",
            "type": "individual",
            "name": "Alice",
            "description": "An engineer",
            "email": "alice@example.com",
            "timeZone": "Europe/London",
            "capabilities": {
                "urn:ietf:params:jmap:mail": {}
            },
            "accounts": {
                "acc1": { "name": "Alice's Account", "isPersonal": true, "isReadOnly": false, "accountCapabilities": {} }
            }
        });
        let p: Principal =
            serde_json::from_value(json).expect("deserialize Principal with accounts");
        assert_eq!(p.description.as_deref(), Some("An engineer"));
        assert!(p.accounts.is_some());
        let accounts = p.accounts.as_ref().unwrap();
        assert_eq!(accounts.len(), 1);
        let acc_id = Id::from("acc1");
        assert!(accounts.contains_key(&acc_id));
    }

    // --- PrincipalType enum tests ---

    #[test]
    fn principal_type_known_values() {
        let cases = [
            ("individual", PrincipalType::Individual),
            ("group", PrincipalType::Group),
            ("resource", PrincipalType::Resource),
            ("location", PrincipalType::Location),
        ];
        for (wire, expected) in &cases {
            let got: PrincipalType =
                serde_json::from_value(json!(wire)).expect("deserialize PrincipalType");
            assert_eq!(&got, expected, "wire value: {wire}");
        }
    }

    /// RFC 9670 §2: `"other"` is a defined value meaning "some other undefined Principal".
    /// It maps to `Other("other")` via the catch-all in `impl_string_enum!`.
    #[test]
    fn principal_type_other_known_value() {
        let got: PrincipalType =
            serde_json::from_value(json!("other")).expect("deserialize 'other'");
        assert_eq!(got, PrincipalType::Other("other".to_owned()));
    }

    #[test]
    fn principal_type_unknown_string_roundtrips() {
        let got: PrincipalType =
            serde_json::from_value(json!("future-type-xyz")).expect("deserialize unknown");
        assert_eq!(got, PrincipalType::Other("future-type-xyz".to_owned()));
        let back = serde_json::to_value(&got).expect("serialize");
        assert_eq!(back, json!("future-type-xyz"));
    }

    #[test]
    fn principal_type_display() {
        assert_eq!(PrincipalType::Individual.to_string(), "individual");
        assert_eq!(PrincipalType::Group.to_string(), "group");
        assert_eq!(PrincipalType::Resource.to_string(), "resource");
        assert_eq!(PrincipalType::Location.to_string(), "location");
        assert_eq!(
            PrincipalType::Other("custom".to_owned()).to_string(),
            "custom"
        );
    }

    // --- PrincipalFilterCondition tests ---

    #[test]
    fn filter_condition_default_is_empty() {
        let fc = PrincipalFilterCondition::default();
        let json = serde_json::to_value(&fc).expect("serialize empty filter");
        assert_eq!(json, json!({}));
    }

    #[test]
    fn filter_condition_roundtrip() {
        let json = json!({
            "name": "Joe",
            "email": "joe@example.com",
            "type": "individual",
            "timeZone": "UTC"
        });
        let fc = PrincipalFilterCondition::deserialize(&json).expect("deserialize filter");
        assert_eq!(fc.name.as_deref(), Some("Joe"));
        assert_eq!(fc.email.as_deref(), Some("joe@example.com"));
        assert_eq!(fc.type_, Some(PrincipalType::Individual));
        assert_eq!(fc.time_zone.as_deref(), Some("UTC"));

        let reserialized = serde_json::to_value(&fc).expect("serialize");
        // Optional fields present in both must match
        assert_eq!(reserialized["name"], json["name"]);
        assert_eq!(reserialized["email"], json["email"]);
        assert_eq!(reserialized["type"], json["type"]);
        assert_eq!(reserialized["timeZone"], json["timeZone"]);
    }

    #[test]
    fn filter_condition_account_ids() {
        let json = json!({ "accountIds": ["acc1", "acc2"] });
        let fc: PrincipalFilterCondition =
            serde_json::from_value(json).expect("deserialize accountIds filter");
        let ids = fc.account_ids.as_ref().expect("accountIds should be Some");
        assert_eq!(ids, &["acc1", "acc2"]);
    }

    #[test]
    fn filter_condition_text_only() {
        let json = json!({ "text": "meeting room" });
        let fc: PrincipalFilterCondition =
            serde_json::from_value(json).expect("deserialize text filter");
        assert_eq!(fc.text.as_deref(), Some("meeting room"));
        assert!(fc.name.is_none());
        assert!(fc.email.is_none());
    }

    /// Unknown `type` values round-trip via PrincipalType::Other for
    /// forward compatibility.
    ///
    /// Oracle: PrincipalType::Other(String) catch-all — unknown future values
    /// must survive a deserialize → serialize round-trip unchanged.
    #[test]
    fn filter_condition_type_other_roundtrip() {
        let json = json!({ "type": "future-unknown-value" });
        let fc: PrincipalFilterCondition =
            serde_json::from_value(json).expect("deserialize filter with unknown type");
        assert_eq!(
            fc.type_,
            Some(PrincipalType::Other("future-unknown-value".to_owned()))
        );
        let serialized = serde_json::to_string(&fc).expect("serialize");
        assert!(
            serialized.contains("future-unknown-value"),
            "round-trip must preserve unknown type string, got: {serialized}"
        );
    }
}
