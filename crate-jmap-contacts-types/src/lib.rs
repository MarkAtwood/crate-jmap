//! RFC 9610 + RFC 9553 data types for JMAP Contacts.
//!
//! Provides [`AddressBook`], [`AddressBookRights`], [`ContactCard`],
//! [`ContactCardFilterCondition`], [`ContactCardComparator`],
//! [`ContactsCapability`], and [`ContactsAccountCapability`].
//!
//! This crate is types-only: no method handlers, no async, no network I/O.
//! It sits between `jmap-types` (shared JMAP base primitives) and the
//! server/client crates that consume these types.
//!
//! All types implement [`serde::Serialize`] and [`serde::Deserialize`] with
//! the camelCase field names required by the JMAP wire format.
//!
//! # Example
//!
//! ```rust
//! use jmap_contacts_types::AddressBook;
//!
//! let json = r#"{
//!     "id": "ab1",
//!     "name": "Personal",
//!     "sortOrder": 0,
//!     "isDefault": true,
//!     "isSubscribed": true,
//!     "shareWith": null,
//!     "myRights": {
//!         "mayRead": true,
//!         "mayWrite": true,
//!         "mayShare": true,
//!         "mayDelete": false
//!     }
//! }"#;
//!
//! let ab: AddressBook = serde_json::from_str(json).unwrap();
//! assert_eq!(ab.name, "Personal");
//! ```

#![forbid(unsafe_code)]

#[macro_use]
mod string_enum;

pub mod addressbook;
pub mod backend;
pub mod capability;
pub mod card;

pub use addressbook::{AddressBook, AddressBookRights};
pub use backend::{AddressBookProperty, ContactCardProperty};
pub use capability::{ContactsAccountCapability, ContactsCapability, JMAP_CONTACTS_URI};
pub use card::{ContactCard, ContactCardComparator, ContactCardFilterCondition};

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use jmap_types::Id;
    use serde_json::json;

    use super::*;

    // ── AddressBookRights ────────────────────────────────────────────────

    /// AddressBookRights::default() MUST produce all-false.
    /// Oracle: spec §2 — four boolean fields, default-false is the safe value.
    #[test]
    fn address_book_rights_default_is_all_false() {
        let r = AddressBookRights::default();
        assert!(!r.may_read, "may_read default should be false");
        assert!(!r.may_write, "may_write default should be false");
        assert!(!r.may_share, "may_share default should be false");
        assert!(!r.may_delete, "may_delete default should be false");
    }

    /// Deserialize AddressBookRights from spec §4.1 example JSON.
    /// Oracle: literal JSON from RFC 9610 §4.1.
    #[test]
    fn address_book_rights_deserialize() {
        let json = r#"{
            "mayRead": true,
            "mayWrite": false,
            "mayShare": false,
            "mayDelete": false
        }"#;
        let r: AddressBookRights = serde_json::from_str(json).unwrap();
        assert!(r.may_read);
        assert!(!r.may_write);
        assert!(!r.may_share);
        assert!(!r.may_delete);
    }

    /// AddressBookRights round-trip: serialize then deserialize must be
    /// identity.
    #[test]
    fn address_book_rights_round_trip() {
        let original = AddressBookRights {
            may_read: true,
            may_write: true,
            may_share: false,
            may_delete: false,
        };
        let json = serde_json::to_string(&original).unwrap();
        let decoded: AddressBookRights = serde_json::from_str(&json).unwrap();
        assert_eq!(original, decoded);
    }

    // ── AddressBook ──────────────────────────────────────────────────────

    /// Deserialize an AddressBook from the spec §4.1 example response.
    /// Oracle: literal JSON from RFC 9610 §4.1.
    #[test]
    fn address_book_deserialize_spec_example() {
        // Taken verbatim from RFC 9610 §4.1.
        let json = r#"{
            "id": "062adcfa-105d-455c-bc60-6db68b69c3f3",
            "name": "Personal",
            "description": null,
            "sortOrder": 0,
            "isDefault": true,
            "isSubscribed": true,
            "shareWith": {
                "3f1502e0-63fe-4335-9ff3-e739c188f5dd": {
                    "mayRead": true,
                    "mayWrite": false,
                    "mayShare": false,
                    "mayDelete": false
                }
            },
            "myRights": {
                "mayRead": true,
                "mayWrite": true,
                "mayShare": true,
                "mayDelete": false
            }
        }"#;
        let ab: AddressBook = serde_json::from_str(json).unwrap();
        assert_eq!(ab.name, "Personal");
        assert!(ab.is_default);
        assert!(ab.is_subscribed);
        assert_eq!(ab.sort_order, 0);
        assert!(ab.description.is_none());

        let share_with = ab.share_with.unwrap();
        let principal_id: Id = Id::from("3f1502e0-63fe-4335-9ff3-e739c188f5dd");
        let shared_rights = &share_with[&principal_id];
        assert!(shared_rights.may_read);
        assert!(!shared_rights.may_write);

        assert!(ab.my_rights.may_read);
        assert!(ab.my_rights.may_write);
        assert!(ab.my_rights.may_share);
        assert!(!ab.my_rights.may_delete);
    }

    /// Deserialize the second AddressBook from §4.1 (shareWith: null).
    /// Oracle: literal JSON from RFC 9610 §4.1.
    #[test]
    fn address_book_share_with_null() {
        let json = r#"{
            "id": "cd40089d-35f9-4fd7-980b-ba3a9f1d74fe",
            "name": "Autosaved",
            "description": null,
            "sortOrder": 1,
            "isDefault": false,
            "isSubscribed": true,
            "shareWith": null,
            "myRights": {
                "mayRead": true,
                "mayWrite": true,
                "mayShare": true,
                "mayDelete": false
            }
        }"#;
        let ab: AddressBook = serde_json::from_str(json).unwrap();
        assert_eq!(ab.name, "Autosaved");
        assert!(ab.share_with.is_none());
    }

    /// AddressBook with share_with: None serializes as "shareWith": null, not absent.
    /// Oracle: RFC 9610 §2 type `Id[AddressBookRights]|null`; §4.1 second
    /// AddressBook example shows `"shareWith": null` explicitly on the wire.
    #[test]
    fn address_book_share_with_null_serializes() {
        // Build an AddressBook that has share_with: None (the null case).
        let json = r#"{
            "id": "ab-null-sw",
            "name": "Private",
            "sortOrder": 0,
            "isDefault": false,
            "isSubscribed": true,
            "shareWith": null,
            "myRights": {
                "mayRead": true,
                "mayWrite": true,
                "mayShare": false,
                "mayDelete": false
            }
        }"#;
        let ab: AddressBook = serde_json::from_str(json).expect("deserialize");
        assert!(ab.share_with.is_none());

        let serialized = serde_json::to_value(&ab).expect("serialize");
        let obj = serialized.as_object().expect("object");
        assert!(
            obj.contains_key("shareWith"),
            "shareWith must be present in wire JSON (required-nullable)"
        );
        assert!(
            obj["shareWith"].is_null(),
            "shareWith must serialize as null when None"
        );
    }

    /// AddressBook with share_with: Some(map) round-trips correctly.
    /// Oracle: RFC 9610 §4.1 first AddressBook example with populated shareWith map.
    #[test]
    fn address_book_share_with_some_serializes() {
        // From RFC 9610 §4.1 first AddressBook.
        let json = r#"{
            "id": "062adcfa-105d-455c-bc60-6db68b69c3f3",
            "name": "Personal",
            "description": null,
            "sortOrder": 0,
            "isDefault": true,
            "isSubscribed": true,
            "shareWith": {
                "3f1502e0-63fe-4335-9ff3-e739c188f5dd": {
                    "mayRead": true,
                    "mayWrite": false,
                    "mayShare": false,
                    "mayDelete": false
                }
            },
            "myRights": {
                "mayRead": true,
                "mayWrite": true,
                "mayShare": true,
                "mayDelete": false
            }
        }"#;
        let ab: AddressBook = serde_json::from_str(json).expect("deserialize");
        assert!(ab.share_with.is_some());

        let serialized = serde_json::to_value(&ab).expect("serialize");
        let obj = serialized.as_object().expect("object");
        assert!(
            obj.contains_key("shareWith"),
            "shareWith must be present when set"
        );
        assert!(
            !obj["shareWith"].is_null(),
            "shareWith must not be null when populated"
        );
        // Round-trip: deserialize the serialized form, verify equality.
        let back: AddressBook = serde_json::from_value(serialized).expect("round-trip deserialize");
        assert_eq!(ab.share_with, back.share_with);
    }

    /// AddressBook round-trip: description Some vs None.
    #[test]
    fn address_book_round_trip_with_description() {
        let rights = AddressBookRights {
            may_read: true,
            may_write: false,
            may_share: false,
            may_delete: false,
        };
        // Construct via JSON (avoids #[non_exhaustive] struct literal restriction).
        let original_json = json!({
            "id": "ab1",
            "name": "Work",
            "description": "Work contacts",
            "sortOrder": 5,
            "isDefault": false,
            "isSubscribed": true,
            "myRights": {
                "mayRead": true,
                "mayWrite": false,
                "mayShare": false,
                "mayDelete": false
            }
        });
        let ab: AddressBook = serde_json::from_value(original_json).unwrap();
        assert_eq!(ab.description.as_deref(), Some("Work contacts"));
        let _rt: AddressBook = serde_json::from_str(&serde_json::to_string(&ab).unwrap()).unwrap();
        // rights field unused directly; just suppress unused-variable warning
        let _ = rights;
    }

    /// description: null → None in Rust → "description": null on the wire.
    ///
    /// Oracle: RFC 9553 + RFC 9610 §4.1 — both example
    /// AddressBook objects show `"description": null` explicitly.  The field
    /// must be present as null, never absent.
    #[test]
    fn address_book_description_null_serializes() {
        let json = r#"{
            "id": "AB1",
            "name": "Personal",
            "description": null,
            "sortOrder": 0,
            "isDefault": false,
            "isSubscribed": false,
            "shareWith": null,
            "myRights": {
                "mayRead": true,
                "mayWrite": true,
                "mayShare": false,
                "mayDelete": false
            }
        }"#;
        let ab: AddressBook = serde_json::from_str(json).expect("deserialize");
        assert!(ab.description.is_none(), "description should be None");

        let out = serde_json::to_value(&ab).expect("serialize");
        assert!(
            out.as_object().expect("object").contains_key("description"),
            "description key must be present in wire JSON"
        );
        assert!(
            out["description"].is_null(),
            "description must serialize as null, not be absent"
        );
    }

    // ── ContactCard ──────────────────────────────────────────────────────

    /// Deserialize the ContactCard from the spec §4.1 example.
    /// Oracle: literal JSON from RFC 9610 §4.1.
    #[test]
    fn contact_card_deserialize_spec_example() {
        // From RFC 9610 §4.1 response.
        let json = r#"{
            "id": "3",
            "addressBookIds": {
                "062adcfa-105d-455c-bc60-6db68b69c3f3": true
            },
            "name": {
                "components": [
                    { "kind": "given", "value": "Joe" },
                    { "kind": "surname", "value": "Bloggs" }
                ],
                "isOrdered": true
            },
            "emails": {
                "0": {
                    "contexts": {
                        "private": true
                    },
                    "address": "joe.bloggs@example.com"
                }
            }
        }"#;
        let card: ContactCard = serde_json::from_str(json).unwrap();

        let id = card.id.as_ref().unwrap();
        assert_eq!(id.as_ref(), "3");

        let ab_ids = card.address_book_ids.as_ref().unwrap();
        let ab_key: Id = Id::from("062adcfa-105d-455c-bc60-6db68b69c3f3");
        assert!(ab_ids[&ab_key]);

        // name is a serde_json::Value
        let name = card.name.as_ref().unwrap();
        assert_eq!(name["isOrdered"], serde_json::Value::Bool(true));
        assert_eq!(name["components"][0]["kind"], "given");
        assert_eq!(name["components"][0]["value"], "Joe");

        // emails is a serde_json::Value
        let emails = card.emails.as_ref().unwrap();
        assert_eq!(emails["0"]["address"], "joe.bloggs@example.com");
    }

    /// ContactCard with kind="group" and members map.
    /// Oracle: hand-written from RFC 9553 §2.1.6 example.
    #[test]
    fn contact_card_group_with_members() {
        let json = r#"{
            "version": "1.0",
            "uid": "urn:uuid:ab4310aa-fa43-11e9-8f0b-362b9e155667",
            "kind": "group",
            "name": { "full": "The Doe family" },
            "members": {
                "urn:uuid:03a0e51f-d1aa-4385-8a53-e29025acd8af": true,
                "urn:uuid:b8767877-b4a1-4c70-9acc-505d3819e519": true
            }
        }"#;
        let card: ContactCard = serde_json::from_str(json).unwrap();
        assert_eq!(card.kind.as_deref(), Some("group"));
        let members = card.members.as_ref().unwrap();
        assert!(members["urn:uuid:03a0e51f-d1aa-4385-8a53-e29025acd8af"]);
        assert_eq!(members.len(), 2);
    }

    /// ContactCard with no optional fields: only id and addressBookIds.
    #[test]
    fn contact_card_minimal_fields() {
        let json = r#"{
            "id": "minimal1",
            "addressBookIds": { "ab1": true }
        }"#;
        let card: ContactCard = serde_json::from_str(json).unwrap();
        assert!(card.version.is_none());
        assert!(card.uid.is_none());
        assert!(card.kind.is_none());
        assert!(card.emails.is_none());
        assert!(card.phones.is_none());
        assert!(card.addresses.is_none());
    }

    /// ContactCard skip_serializing_if omits None fields from output.
    #[test]
    fn contact_card_serializes_only_present_fields() {
        // Build from JSON to bypass #[non_exhaustive] struct literal restriction.
        let card: ContactCard = serde_json::from_value(json!({
            "id": "x1",
            "addressBookIds": { "ab1": true },
            "version": "1.0",
            "uid": "urn:uuid:f81d4fae-7dec-11d0-a765-00a0c91e6bf6"
        }))
        .unwrap();

        let serialized = serde_json::to_value(&card).unwrap();
        // Only present fields should appear.
        assert!(serialized.get("id").is_some());
        assert!(serialized.get("version").is_some());
        assert!(serialized.get("uid").is_some());
        // Absent fields must NOT appear.
        assert!(serialized.get("emails").is_none());
        assert!(serialized.get("phones").is_none());
        assert!(serialized.get("addresses").is_none());
        assert!(serialized.get("kind").is_none());
    }

    // ── ContactCardFilterCondition ───────────────────────────────────────

    /// Verify that slash-keyed filter fields serialize with the correct wire
    /// names.
    /// Oracle: RFC 9610 §3.3.1 field list.
    #[test]
    fn filter_condition_slash_keys_serialize_correctly() {
        let filter: ContactCardFilterCondition = serde_json::from_value(json!({
            "name/given": "John",
            "name/surname": "Smith",
            "name/surname2": "Doe"
        }))
        .unwrap();

        assert_eq!(filter.name_given.as_deref(), Some("John"));
        assert_eq!(filter.name_surname.as_deref(), Some("Smith"));
        assert_eq!(filter.name_surname2.as_deref(), Some("Doe"));

        let serialized = serde_json::to_value(&filter).unwrap();
        assert_eq!(serialized["name/given"], "John");
        assert_eq!(serialized["name/surname"], "Smith");
        assert_eq!(serialized["name/surname2"], "Doe");
    }

    /// All filter fields deserialize from spec §3.3.1 field names.
    /// Oracle: field names taken directly from RFC 9610 §3.3.1.
    #[test]
    fn filter_condition_all_fields() {
        let json = r#"{
            "inAddressBook": "ab1",
            "uid": "urn:uuid:abc123",
            "hasMember": "urn:uuid:def456",
            "kind": "individual",
            "createdBefore": "2024-01-01T00:00:00Z",
            "createdAfter": "2020-01-01T00:00:00Z",
            "updatedBefore": "2024-06-01T00:00:00Z",
            "updatedAfter": "2021-06-01T00:00:00Z",
            "text": "John",
            "name": "Doe",
            "name/given": "Jane",
            "name/surname": "Smith",
            "name/surname2": "Gomez",
            "nickname": "Johnny",
            "organization": "ACME",
            "email": "john@example.com",
            "phone": "+1-555-1234",
            "onlineService": "twitter",
            "address": "Main St",
            "note": "VIP"
        }"#;
        let f: ContactCardFilterCondition = serde_json::from_str(json).unwrap();
        assert_eq!(
            f.in_address_book.as_ref().map(|id| id.as_ref()),
            Some("ab1")
        );
        assert_eq!(f.uid.as_deref(), Some("urn:uuid:abc123"));
        assert_eq!(f.has_member.as_deref(), Some("urn:uuid:def456"));
        assert_eq!(f.kind.as_deref(), Some("individual"));
        assert_eq!(f.created_before.as_deref(), Some("2024-01-01T00:00:00Z"));
        assert_eq!(f.created_after.as_deref(), Some("2020-01-01T00:00:00Z"));
        assert_eq!(f.updated_before.as_deref(), Some("2024-06-01T00:00:00Z"));
        assert_eq!(f.updated_after.as_deref(), Some("2021-06-01T00:00:00Z"));
        assert_eq!(f.text.as_deref(), Some("John"));
        assert_eq!(f.name.as_deref(), Some("Doe"));
        assert_eq!(f.name_given.as_deref(), Some("Jane"));
        assert_eq!(f.name_surname.as_deref(), Some("Smith"));
        assert_eq!(f.name_surname2.as_deref(), Some("Gomez"));
        assert_eq!(f.nickname.as_deref(), Some("Johnny"));
        assert_eq!(f.organization.as_deref(), Some("ACME"));
        assert_eq!(f.email.as_deref(), Some("john@example.com"));
        assert_eq!(f.phone.as_deref(), Some("+1-555-1234"));
        assert_eq!(f.online_service.as_deref(), Some("twitter"));
        assert_eq!(f.address.as_deref(), Some("Main St"));
        assert_eq!(f.note.as_deref(), Some("VIP"));
    }

    /// Empty filter condition serializes to `{}`.
    #[test]
    fn filter_condition_empty_serializes_to_empty_object() {
        let f = ContactCardFilterCondition::default();
        let v = serde_json::to_value(&f).unwrap();
        assert_eq!(v, json!({}));
    }

    // ── Capability ───────────────────────────────────────────────────────

    /// ContactsCapability serializes to `{}`.
    /// Oracle: RFC 9610 §1.4.1 — session-level capability is an empty object.
    #[test]
    fn contacts_capability_is_empty_object() {
        let cap = ContactsCapability::default();
        let v = serde_json::to_value(&cap).unwrap();
        assert_eq!(v, json!({}));
    }

    /// ContactsAccountCapability deserializes correctly.
    /// Oracle: RFC 9610 §1.4.1 field list.
    #[test]
    fn contacts_account_capability_deserialize() {
        let json = r#"{
            "maxAddressBooksPerCard": 5,
            "mayCreateAddressBook": true
        }"#;
        let cap: ContactsAccountCapability = serde_json::from_str(json).unwrap();
        assert_eq!(cap.max_address_books_per_card, Some(5));
        assert!(cap.may_create_address_book);
    }

    /// ContactsAccountCapability with null maxAddressBooksPerCard.
    /// Oracle: RFC 9610 §1.4.1 — "null for no limit".
    #[test]
    fn contacts_account_capability_null_max() {
        let json = r#"{
            "maxAddressBooksPerCard": null,
            "mayCreateAddressBook": false
        }"#;
        let cap: ContactsAccountCapability = serde_json::from_str(json).unwrap();
        assert!(cap.max_address_books_per_card.is_none());
        assert!(!cap.may_create_address_book);
    }

    /// URI constant has the correct value.
    #[test]
    fn jmap_contacts_uri_value() {
        assert_eq!(JMAP_CONTACTS_URI, "urn:ietf:params:jmap:contacts");
    }

    // ── ContactCardComparator ────────────────────────────────────────────

    /// is_ascending defaults to true when absent from JSON.
    /// Oracle: JMAP base protocol default comparator behavior.
    #[test]
    fn comparator_is_ascending_defaults_to_true() {
        let json = r#"{ "property": "created" }"#;
        let c: ContactCardComparator = serde_json::from_str(json).unwrap();
        assert!(c.is_ascending);
        assert!(c.collation.is_none());
    }

    /// Comparator round-trip with all fields.
    #[test]
    fn comparator_round_trip() {
        let json = r#"{ "property": "name/given", "isAscending": false, "collation": "i;unicode-casemap" }"#;
        let c: ContactCardComparator = serde_json::from_str(json).unwrap();
        assert_eq!(c.property, "name/given");
        assert!(!c.is_ascending);
        assert_eq!(c.collation.as_deref(), Some("i;unicode-casemap"));
        let rt: ContactCardComparator =
            serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(rt.property, "name/given");
        assert!(!rt.is_ascending);
    }

    // ── Suppress unused-import warnings ─────────────────────────────────
    fn _use_imports() {
        let _: HashMap<String, bool> = HashMap::new();
    }
}
