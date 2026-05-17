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
//! ## JSContact sub-object types
//!
//! The RFC 9553 JSContact sub-object types ([`Name`], [`EmailAddress`],
//! [`Phone`], [`Address`], etc.) live in the dedicated
//! `jmap-jscontact-types` crate and are re-exported here for caller
//! ergonomics. The [`ContactCard`] struct keeps its 22 sub-object fields
//! typed as `Option<serde_json::Value>` as the wire-format anchor — typed
//! access is opt-in via [`serde_json::from_value`] on a chosen field.
//!
//! ```rust
//! use jmap_contacts_types::{ContactCard, Name};
//!
//! let card_json = serde_json::json!({
//!     "name": {
//!         "components": [
//!             { "kind": "given", "value": "Vincent" },
//!             { "kind": "surname", "value": "van Gogh" }
//!         ],
//!         "isOrdered": true
//!     }
//! });
//! let card: ContactCard = serde_json::from_value(card_json).unwrap();
//! let name: Name = serde_json::from_value(card.name.unwrap()).unwrap();
//! assert_eq!(name.components.unwrap()[0].value, "Vincent");
//! ```
//!
//! # Logging safety
//!
//! This crate derives plain [`Debug`] on all PII-bearing types
//! ([`ContactCard`], [`AddressBook`], and the 30+ re-exported RFC 9553
//! JSContact sub-types: [`Name`], [`EmailAddress`], [`Phone`],
//! [`Address`], [`Note`], [`Anniversary`], [`PersonalInfo`],
//! [`CryptoKey`], etc.). Consumers using `tracing` or `log` macros
//! that interpolate values via `?` (debug) or `%` (display) emit the
//! full record contents to the configured sink. This includes phone
//! numbers, email addresses, postal addresses, names, notes, and
//! CryptoKey URIs — all of which are typically regulated as
//! personally-identifying information under GDPR Article 5, CCPA
//! §1798.100, and similar privacy frameworks.
//!
//! Consumers SHOULD wrap PII-bearing values in a redacting newtype
//! before logging, or use a structured-logging field-allowlist:
//!
//! ```ignore
//! // BAD — leaks every name, phone, email, address into the log sink.
//! tracing::debug!(card = ?contact_card, "fetched contact");
//!
//! // GOOD — emits only the opaque id and surfaces nothing else.
//! tracing::debug!(card_id = %contact_card.id.as_deref().unwrap_or_default(), "fetched contact");
//! ```
//!
//! The workspace has a Debug-redaction precedent in `jmap-base-client`
//! for credential-bearing types ([`BearerAuth`], [`BasicAuth`],
//! `Session`, `AccountInfo`) but **not** for PII-bearing types. The
//! decision whether to propagate that precedent workspace-wide is
//! tracked by a separate workspace-policy bead; this crate's current
//! posture is plain `Debug` plus this safety contract. JMAP-glx8.26.
//!
//! [`BearerAuth`]: https://docs.rs/jmap-base-client
//! [`BasicAuth`]: https://docs.rs/jmap-base-client
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

pub mod addressbook;
pub mod backend;
pub mod capability;
pub mod card;
pub mod collision;

/// Module alias re-exporting [`jmap_jscontact_types`].
///
/// Provides nested access to JSContact sub-object types as
/// `jmap_contacts_types::jscontact::Name`, mirroring the
/// `jmap_calendars_types::jscalendar::*` pattern. New code should prefer
/// the top-level re-exports (e.g. `jmap_contacts_types::Name`) or the
/// direct path `jmap_jscontact_types::Name`.
pub use jmap_jscontact_types as jscontact;

pub use addressbook::{AddressBook, AddressBookRights};
pub use backend::{AddressBookProperty, ContactCardProperty};
pub use capability::{ContactsAccountCapability, ContactsCapability, JMAP_CONTACTS_URI};
pub use card::{ContactCard, ContactCardComparator, ContactCardFilter, ContactCardFilterCondition};
pub use collision::CollisionError;

/// Constant identifiers for [`ContactCardComparator::property`] sort keys
/// (re-export of [`card::prop`]).
pub use card::prop as comparator_prop;

/// Generic filter algebra from `jmap-types::query` (RFC 8620 §5.5).
///
/// Re-exported here so callers of `jmap-contacts-types` do not need a
/// direct dependency on `jmap-types`. Mirrors the canonical
/// [`jmap_mail_types::query`] re-exports from the workspace canonical
/// extension-types template.
///
/// [`jmap_mail_types::query`]: https://docs.rs/jmap-mail-types/latest/jmap_mail_types/query/index.html
pub use jmap_types::query::{Filter, FilterOperator, Operator};

// ── JSContact sub-object re-exports (RFC 9553) ───────────────────────────────
//
// Mirrors the jmap-calendars-types re-export pattern. Top-level access to
// every public RFC 9553 typed sub-object so callers can write
// `jmap_contacts_types::Name` symmetrically with the wire field name on
// `ContactCard`.
pub use jmap_jscontact_types::{
    Address, AddressComponent, Anniversary, AnniversaryDate, Author, Calendar, CryptoKey,
    Directory, EmailAddress, LanguagePref, Link, Media, Name, NameComponent, Nickname, Note,
    OnlineService, OrgUnit, Organization, PartialDate, PersonalInfo, Phone, Pronouns, Relation,
    SchedulingAddress, SpeakToAs, Timestamp, Title,
};

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
            extra: serde_json::Map::new(),
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
        let rt: AddressBook = serde_json::from_str(&serde_json::to_string(&ab).unwrap()).unwrap();
        assert_eq!(rt.description.as_deref(), Some("Work contacts"));
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

    /// `localizations` round-trips as a `HashMap<String, PatchObject>` and
    /// is byte-identical on the wire (proving `#[serde(transparent)]` on
    /// `PatchObject` introduces no wrapper key).
    ///
    /// Oracle: hand-written JSON modelled on RFC 9553 §2.7.1 localizations
    /// (BCP 47 language tag keys → PatchObject values).
    #[test]
    fn contact_card_localizations_patch_object_transparent() {
        let json = r#"{"localizations":{"de":{"name/full":"Joe Bloggs (DE)"}}}"#;
        let card: ContactCard = serde_json::from_str(json).expect("deserialize");

        let locs = card.localizations.as_ref().expect("localizations present");
        let de = locs.get("de").expect("de locale present");
        assert_eq!(
            de.as_map().get("name/full").and_then(|v| v.as_str()),
            Some("Joe Bloggs (DE)"),
        );

        let serialized = serde_json::to_string(&card).expect("serialize");
        assert_eq!(serialized, json);
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
        assert_eq!(
            f.created_before.as_ref().map(AsRef::as_ref),
            Some("2024-01-01T00:00:00Z")
        );
        assert_eq!(
            f.created_after.as_ref().map(AsRef::as_ref),
            Some("2020-01-01T00:00:00Z")
        );
        assert_eq!(
            f.updated_before.as_ref().map(AsRef::as_ref),
            Some("2024-06-01T00:00:00Z")
        );
        assert_eq!(
            f.updated_after.as_ref().map(AsRef::as_ref),
            Some("2021-06-01T00:00:00Z")
        );
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

    // ── ContactCard sloppy-field → jmap-jscontact-types typed round-trip ──
    //
    // RFC 9610 §3 defers all RFC 9553 sub-object shapes to the JSContact
    // spec. The sloppy `Option<serde_json::Value>` fields on `ContactCard`
    // are the wire-format anchor; consumers obtain typed views via
    // `serde_json::from_value` into the corresponding type from
    // `jmap-jscontact-types`. The tests below cover each sloppy field
    // against RFC 9553 example JSON, mirroring the per-field acceptance
    // criterion in bd:JMAP-sehw.2.
    //
    // Oracle: hand-typed JSON taken from RFC 9553 worked examples.

    /// Deserialize the sloppy `name` field through `Name`.
    /// Oracle: RFC 9553 Figure 16.
    #[test]
    fn sloppy_field_name_roundtrips_through_jscontact_name() {
        let card_json = json!({
            "name": {
                "components": [
                    { "kind": "given", "value": "Vincent" },
                    { "kind": "surname", "value": "van Gogh" }
                ],
                "isOrdered": true
            }
        });
        let card: ContactCard = serde_json::from_value(card_json).unwrap();
        let name: Name = serde_json::from_value(card.name.clone().unwrap()).unwrap();
        let back = serde_json::to_value(&name).unwrap();
        assert_eq!(back, card.name.unwrap());
    }

    /// Deserialize the sloppy `nicknames` field through `HashMap<String, Nickname>`.
    /// Oracle: RFC 9553 Figure 21.
    #[test]
    fn sloppy_field_nicknames_roundtrips_through_jscontact_nickname_map() {
        let card_json = json!({
            "nicknames": {
                "k391": { "name": "Johnny" }
            }
        });
        let card: ContactCard = serde_json::from_value(card_json).unwrap();
        let nicks: HashMap<String, Nickname> =
            serde_json::from_value(card.nicknames.clone().unwrap()).unwrap();
        assert_eq!(nicks["k391"].name, "Johnny");
        let back = serde_json::to_value(&nicks).unwrap();
        assert_eq!(back, card.nicknames.unwrap());
    }

    /// Deserialize the sloppy `organizations` field through `HashMap<String, Organization>`.
    /// Oracle: RFC 9553 Figure 22.
    #[test]
    fn sloppy_field_organizations_roundtrips_through_jscontact_organization_map() {
        let card_json = json!({
            "organizations": {
                "o1": {
                    "name": "ABC, Inc.",
                    "units": [
                        { "name": "North American Division" },
                        { "name": "Marketing" }
                    ],
                    "sortAs": "ABC"
                }
            }
        });
        let card: ContactCard = serde_json::from_value(card_json).unwrap();
        let orgs: HashMap<String, Organization> =
            serde_json::from_value(card.organizations.clone().unwrap()).unwrap();
        assert_eq!(orgs["o1"].name.as_deref(), Some("ABC, Inc."));
        let back = serde_json::to_value(&orgs).unwrap();
        assert_eq!(back, card.organizations.unwrap());
    }

    /// Deserialize the sloppy `speakToAs` field through `SpeakToAs`.
    /// Oracle: RFC 9553 Figure 23.
    #[test]
    fn sloppy_field_speak_to_as_roundtrips_through_jscontact_speak_to_as() {
        let card_json = json!({
            "speakToAs": {
                "grammaticalGender": "neuter",
                "pronouns": {
                    "k19": { "pronouns": "they/them", "pref": 2 }
                }
            }
        });
        let card: ContactCard = serde_json::from_value(card_json).unwrap();
        let speak: SpeakToAs = serde_json::from_value(card.speak_to_as.clone().unwrap()).unwrap();
        let back = serde_json::to_value(&speak).unwrap();
        assert_eq!(back, card.speak_to_as.unwrap());
    }

    /// Deserialize the sloppy `titles` field through `HashMap<String, Title>`.
    /// Oracle: RFC 9553 Figure 24.
    #[test]
    fn sloppy_field_titles_roundtrips_through_jscontact_title_map() {
        let card_json = json!({
            "titles": {
                "le9": { "kind": "title", "name": "Research Scientist" }
            }
        });
        let card: ContactCard = serde_json::from_value(card_json).unwrap();
        let titles: HashMap<String, Title> =
            serde_json::from_value(card.titles.clone().unwrap()).unwrap();
        assert_eq!(titles["le9"].name, "Research Scientist");
        let back = serde_json::to_value(&titles).unwrap();
        assert_eq!(back, card.titles.unwrap());
    }

    /// Deserialize the sloppy `emails` field through `HashMap<String, EmailAddress>`.
    /// Oracle: RFC 9553 Figure 25.
    #[test]
    fn sloppy_field_emails_roundtrips_through_jscontact_email_address_map() {
        let card_json = json!({
            "emails": {
                "e1": { "contexts": { "work": true }, "address": "jqpublic@xyz.example.com" }
            }
        });
        let card: ContactCard = serde_json::from_value(card_json).unwrap();
        let emails: HashMap<String, EmailAddress> =
            serde_json::from_value(card.emails.clone().unwrap()).unwrap();
        assert_eq!(emails["e1"].address, "jqpublic@xyz.example.com");
        let back = serde_json::to_value(&emails).unwrap();
        assert_eq!(back, card.emails.unwrap());
    }

    /// Deserialize the sloppy `onlineServices` field through `HashMap<String, OnlineService>`.
    /// Oracle: RFC 9553 Figure 26.
    #[test]
    fn sloppy_field_online_services_roundtrips_through_jscontact_online_service_map() {
        let card_json = json!({
            "onlineServices": {
                "x1": { "uri": "xmpp:alice@example.com" }
            }
        });
        let card: ContactCard = serde_json::from_value(card_json).unwrap();
        let svcs: HashMap<String, OnlineService> =
            serde_json::from_value(card.online_services.clone().unwrap()).unwrap();
        let back = serde_json::to_value(&svcs).unwrap();
        assert_eq!(back, card.online_services.unwrap());
    }

    /// Deserialize the sloppy `phones` field through `HashMap<String, Phone>`.
    /// Oracle: RFC 9553 Figure 27.
    #[test]
    fn sloppy_field_phones_roundtrips_through_jscontact_phone_map() {
        let card_json = json!({
            "phones": {
                "tel0": {
                    "contexts": { "private": true },
                    "features": { "voice": true },
                    "number": "tel:+1-555-555-5555;ext=5555",
                    "pref": 1
                }
            }
        });
        let card: ContactCard = serde_json::from_value(card_json).unwrap();
        let phones: HashMap<String, Phone> =
            serde_json::from_value(card.phones.clone().unwrap()).unwrap();
        assert_eq!(phones["tel0"].number, "tel:+1-555-555-5555;ext=5555");
        let back = serde_json::to_value(&phones).unwrap();
        assert_eq!(back, card.phones.unwrap());
    }

    /// Deserialize the sloppy `preferredLanguages` field through `HashMap<String, LanguagePref>`.
    /// Oracle: RFC 9553 Figure 28.
    #[test]
    fn sloppy_field_preferred_languages_roundtrips_through_jscontact_language_pref_map() {
        let card_json = json!({
            "preferredLanguages": {
                "l1": { "language": "en", "contexts": { "work": true }, "pref": 1 }
            }
        });
        let card: ContactCard = serde_json::from_value(card_json).unwrap();
        let langs: HashMap<String, LanguagePref> =
            serde_json::from_value(card.preferred_languages.clone().unwrap()).unwrap();
        assert_eq!(langs["l1"].language, "en");
        let back = serde_json::to_value(&langs).unwrap();
        assert_eq!(back, card.preferred_languages.unwrap());
    }

    /// Deserialize the sloppy `calendars` field through `HashMap<String, Calendar>`.
    /// Oracle: RFC 9553 Figure 29.
    #[test]
    fn sloppy_field_calendars_roundtrips_through_jscontact_calendar_map() {
        let card_json = json!({
            "calendars": {
                "calA": { "kind": "calendar", "uri": "webcal://calendar.example.com/calA.ics" }
            }
        });
        let card: ContactCard = serde_json::from_value(card_json).unwrap();
        let cals: HashMap<String, Calendar> =
            serde_json::from_value(card.calendars.clone().unwrap()).unwrap();
        let back = serde_json::to_value(&cals).unwrap();
        assert_eq!(back, card.calendars.unwrap());
    }

    /// Deserialize the sloppy `schedulingAddresses` field through `HashMap<String, SchedulingAddress>`.
    /// Oracle: RFC 9553 Figure 30.
    #[test]
    fn sloppy_field_scheduling_addresses_roundtrips_through_jscontact_scheduling_address_map() {
        let card_json = json!({
            "schedulingAddresses": {
                "sched1": { "uri": "mailto:janedoe@example.com" }
            }
        });
        let card: ContactCard = serde_json::from_value(card_json).unwrap();
        let scheds: HashMap<String, SchedulingAddress> =
            serde_json::from_value(card.scheduling_addresses.clone().unwrap()).unwrap();
        assert_eq!(scheds["sched1"].uri, "mailto:janedoe@example.com");
        let back = serde_json::to_value(&scheds).unwrap();
        assert_eq!(back, card.scheduling_addresses.unwrap());
    }

    /// Deserialize the sloppy `addresses` field through `HashMap<String, Address>`.
    /// Oracle: RFC 9553 Figure 31.
    #[test]
    fn sloppy_field_addresses_roundtrips_through_jscontact_address_map() {
        let card_json = json!({
            "addresses": {
                "k23": {
                    "contexts": { "work": true },
                    "components": [
                        { "kind": "number", "value": "54321" },
                        { "kind": "name", "value": "Oak St" }
                    ],
                    "countryCode": "US",
                    "isOrdered": true
                }
            }
        });
        let card: ContactCard = serde_json::from_value(card_json).unwrap();
        let addrs: HashMap<String, Address> =
            serde_json::from_value(card.addresses.clone().unwrap()).unwrap();
        assert_eq!(addrs["k23"].country_code.as_deref(), Some("US"));
        let back = serde_json::to_value(&addrs).unwrap();
        assert_eq!(back, card.addresses.unwrap());
    }

    /// Deserialize the sloppy `cryptoKeys` field through `HashMap<String, CryptoKey>`.
    /// Oracle: RFC 9553 Figure 34.
    #[test]
    fn sloppy_field_crypto_keys_roundtrips_through_jscontact_crypto_key_map() {
        let card_json = json!({
            "cryptoKeys": {
                "mykey1": { "uri": "https://www.example.com/keys/jdoe.cer" }
            }
        });
        let card: ContactCard = serde_json::from_value(card_json).unwrap();
        let keys: HashMap<String, CryptoKey> =
            serde_json::from_value(card.crypto_keys.clone().unwrap()).unwrap();
        let back = serde_json::to_value(&keys).unwrap();
        assert_eq!(back, card.crypto_keys.unwrap());
    }

    /// Deserialize the sloppy `directories` field through `HashMap<String, Directory>`.
    /// Oracle: RFC 9553 Figure 36.
    #[test]
    fn sloppy_field_directories_roundtrips_through_jscontact_directory_map() {
        let card_json = json!({
            "directories": {
                "dir1": {
                    "kind": "entry",
                    "uri": "https://dir.example.com/addrbook/jdoe/Jean%20Dupont.vcf"
                }
            }
        });
        let card: ContactCard = serde_json::from_value(card_json).unwrap();
        let dirs: HashMap<String, Directory> =
            serde_json::from_value(card.directories.clone().unwrap()).unwrap();
        assert_eq!(dirs["dir1"].kind.as_deref(), Some("entry"));
        let back = serde_json::to_value(&dirs).unwrap();
        assert_eq!(back, card.directories.unwrap());
    }

    /// Deserialize the sloppy `links` field through `HashMap<String, Link>`.
    /// Oracle: RFC 9553 Figure 37.
    #[test]
    fn sloppy_field_links_roundtrips_through_jscontact_link_map() {
        let card_json = json!({
            "links": {
                "link3": { "kind": "contact", "uri": "mailto:contact@example.com", "pref": 1 }
            }
        });
        let card: ContactCard = serde_json::from_value(card_json).unwrap();
        let links: HashMap<String, Link> =
            serde_json::from_value(card.links.clone().unwrap()).unwrap();
        let back = serde_json::to_value(&links).unwrap();
        assert_eq!(back, card.links.unwrap());
    }

    /// Deserialize the sloppy `media` field through `HashMap<String, Media>`.
    /// Oracle: RFC 9553 Figure 38.
    #[test]
    fn sloppy_field_media_roundtrips_through_jscontact_media_map() {
        let card_json = json!({
            "media": {
                "res47": { "kind": "logo", "uri": "https://www.example.com/pub/logos/abccorp.jpg" }
            }
        });
        let card: ContactCard = serde_json::from_value(card_json).unwrap();
        let media: HashMap<String, Media> =
            serde_json::from_value(card.media.clone().unwrap()).unwrap();
        assert_eq!(media["res47"].kind.as_deref(), Some("logo"));
        let back = serde_json::to_value(&media).unwrap();
        assert_eq!(back, card.media.unwrap());
    }

    /// Deserialize the sloppy `anniversaries` field through `HashMap<String, Anniversary>`.
    /// Oracle: RFC 9553 Figure 41.
    #[test]
    fn sloppy_field_anniversaries_roundtrips_through_jscontact_anniversary_map() {
        let card_json = json!({
            "anniversaries": {
                "k8": {
                    "kind": "birth",
                    "date": { "year": 1953, "month": 4, "day": 15 }
                }
            }
        });
        let card: ContactCard = serde_json::from_value(card_json).unwrap();
        let annivs: HashMap<String, Anniversary> =
            serde_json::from_value(card.anniversaries.clone().unwrap()).unwrap();
        assert_eq!(annivs["k8"].kind, "birth");
        let back = serde_json::to_value(&annivs).unwrap();
        assert_eq!(back, card.anniversaries.unwrap());
    }

    /// Deserialize the sloppy `notes` field through `HashMap<String, Note>`.
    /// Oracle: RFC 9553 Figure 43.
    #[test]
    fn sloppy_field_notes_roundtrips_through_jscontact_note_map() {
        let card_json = json!({
            "notes": {
                "n1": {
                    "note": "Open office hours are 1600 to 1715 EST, Mon-Fri",
                    "created": "2022-11-23T15:01:32Z",
                    "author": { "name": "John" }
                }
            }
        });
        let card: ContactCard = serde_json::from_value(card_json).unwrap();
        let notes: HashMap<String, Note> =
            serde_json::from_value(card.notes.clone().unwrap()).unwrap();
        assert_eq!(
            notes["n1"].author.as_ref().unwrap().name.as_deref(),
            Some("John")
        );
        let back = serde_json::to_value(&notes).unwrap();
        assert_eq!(back, card.notes.unwrap());
    }

    /// Deserialize the sloppy `personalInfo` field through `HashMap<String, PersonalInfo>`.
    /// Oracle: RFC 9553 Figure 44.
    #[test]
    fn sloppy_field_personal_info_roundtrips_through_jscontact_personal_info_map() {
        let card_json = json!({
            "personalInfo": {
                "pi2": { "kind": "expertise", "value": "chemistry", "level": "high" }
            }
        });
        let card: ContactCard = serde_json::from_value(card_json).unwrap();
        let info: HashMap<String, PersonalInfo> =
            serde_json::from_value(card.personal_info.clone().unwrap()).unwrap();
        assert_eq!(info["pi2"].kind, "expertise");
        let back = serde_json::to_value(&info).unwrap();
        assert_eq!(back, card.personal_info.unwrap());
    }

    /// Deserialize the sloppy `relatedTo` field through `HashMap<String, Relation>`.
    /// Oracle: RFC 9553 Figure 13.
    #[test]
    fn sloppy_field_related_to_roundtrips_through_jscontact_relation_map() {
        let card_json = json!({
            "relatedTo": {
                "urn:uuid:f81d4fae-7dec-11d0-a765-00a0c91e6bf6": {
                    "relation": { "friend": true }
                }
            }
        });
        let card: ContactCard = serde_json::from_value(card_json).unwrap();
        let rels: HashMap<String, Relation> =
            serde_json::from_value(card.related_to.clone().unwrap()).unwrap();
        let back = serde_json::to_value(&rels).unwrap();
        assert_eq!(back, card.related_to.unwrap());
    }

    /// Typed `keywords` field (RFC 9553 §2.8.2 `String[Boolean]`) round-trips.
    /// Oracle: RFC 9553 Figure 42.
    ///
    /// RFC 9553 §2.8.2 declares `keywords` as `String[Boolean]` — there is
    /// no JSContact object type for keywords, only a tag→true map. The
    /// field is typed as `Option<HashMap<String, bool>>` to match the
    /// sibling `members` and `address_book_ids` fields and the spec's
    /// closed shape (JMAP-glx8.3).
    #[test]
    fn keywords_roundtrips_as_string_bool_map() {
        let card_json = json!({
            "keywords": {
                "internet": true,
                "IETF": true
            }
        });
        let card: ContactCard = serde_json::from_value(card_json).unwrap();
        let kws = card.keywords.as_ref().expect("keywords present");
        assert!(kws["internet"]);
        assert!(kws["IETF"]);
        assert_eq!(kws.len(), 2);
    }

    /// Verify the `jscontact` module alias resolves to the same crate as
    /// the top-level re-exports.
    ///
    /// Type identity is asserted at **compile time** by the const fn
    /// pointers below: if `jmap_jscontact_types::Name`,
    /// `crate::jscontact::Name`, and the top-level `Name` re-export ever
    /// resolved to distinct types, these `const _ASSERT_*` items would
    /// fail to typecheck and the build would break — no runtime call
    /// needed. JMAP-glx8.21.
    #[test]
    fn jscontact_module_alias_is_jmap_jscontact_types() {
        const _ASSERT_ALIASED_SAME_AS_DIRECT: fn(
            jmap_jscontact_types::Name,
        ) -> crate::jscontact::Name = |x| x;
        const _ASSERT_TOP_LEVEL_SAME_AS_DIRECT: fn(jmap_jscontact_types::Name) -> Name = |x| x;

        // Smoke-test that all three paths actually deserialize the same
        // wire JSON; a regression in re-export visibility would show up
        // as a build error here, not a runtime panic.
        let v = json!({ "full": "Vincent van Gogh" });
        let _direct: jmap_jscontact_types::Name = serde_json::from_value(v.clone()).unwrap();
        let _aliased: crate::jscontact::Name = serde_json::from_value(v.clone()).unwrap();
        let _top_level: Name = serde_json::from_value(v).unwrap();
    }

    // ── Extras-preservation policy tests (JMAP-lbdy.5) ───────────────────

    /// `AddressBookRights.extra` captures vendor fields and preserves them.
    #[test]
    fn address_book_rights_preserves_vendor_extras() {
        let raw = json!({
            "mayRead": true,
            "mayWrite": false,
            "mayShare": false,
            "mayDelete": false,
            "acmeCorpMayMerge": true
        });
        let r: AddressBookRights = serde_json::from_value(raw).unwrap();
        assert_eq!(
            r.extra.get("acmeCorpMayMerge").and_then(|v| v.as_bool()),
            Some(true)
        );
        let back = serde_json::to_value(&r).unwrap();
        assert_eq!(back["acmeCorpMayMerge"], true);
    }

    /// `AddressBook.extra` captures vendor fields and preserves them.
    #[test]
    fn address_book_preserves_vendor_extras() {
        let raw = json!({
            "id": "ab1",
            "name": "Personal",
            "description": null,
            "sortOrder": 0,
            "isDefault": true,
            "isSubscribed": true,
            "shareWith": null,
            "myRights": {
                "mayRead": true, "mayWrite": true,
                "mayShare": false, "mayDelete": false
            },
            "acmeCorpRetentionDays": 365
        });
        let ab: AddressBook = serde_json::from_value(raw).unwrap();
        assert_eq!(
            ab.extra
                .get("acmeCorpRetentionDays")
                .and_then(|v| v.as_u64()),
            Some(365)
        );
        let back = serde_json::to_value(&ab).unwrap();
        assert_eq!(back["acmeCorpRetentionDays"], 365);
    }

    /// `ContactCard.extra` captures vendor fields and preserves them.
    #[test]
    fn contact_card_preserves_vendor_extras() {
        let raw = json!({
            "uid": "card-1",
            "version": "1.0",
            "acmeCorpExternalId": "ldap-42"
        });
        let c: ContactCard = serde_json::from_value(raw).unwrap();
        assert_eq!(
            c.extra.get("acmeCorpExternalId").and_then(|v| v.as_str()),
            Some("ldap-42")
        );
        let back = serde_json::to_value(&c).unwrap();
        assert_eq!(back["acmeCorpExternalId"], "ldap-42");
    }

    // ── MUST-be-true map values (JMAP-glx8.11) ───────────────────────────
    //
    // `addressBookIds`, `members`, and `keywords` are JMAP `Id[Boolean]` /
    // `String[Boolean]` sets where the spec requires each value MUST be
    // `true` (RFC 9610 §3, RFC 9553 §2.1.6, RFC 9553 §2.8.2). The type
    // shape `HashMap<_, bool>` cannot enforce that — caller / server is
    // responsible for rejecting `false`. The tests below pin the
    // deserialise-cleanly contract so handler-layer validators know what
    // they will see on the wire, matching the canonical `Email.mailbox_ids`
    // precedent in `jmap-mail-types`.

    /// `addressBookIds` with `false` deserialises cleanly; the type does
    /// not enforce MUST-be-true. Oracle: hand-built JSON.
    #[test]
    fn contact_card_address_book_ids_accepts_false_deserialise() {
        let raw = json!({
            "id": "card-1",
            "addressBookIds": { "ab1": false }
        });
        let c: ContactCard = serde_json::from_value(raw).unwrap();
        let ids = c.address_book_ids.as_ref().unwrap();
        let ab1: Id = Id::from("ab1");
        // false is preserved on round-trip — handler layer rejects, not the type.
        assert_eq!(ids.get(&ab1).copied(), Some(false));
    }

    /// `members` with `false` deserialises cleanly; the type does not
    /// enforce MUST-be-true. Oracle: hand-built JSON.
    #[test]
    fn contact_card_members_accepts_false_deserialise() {
        let raw = json!({
            "id": "card-1",
            "kind": "group",
            "members": { "urn:uuid:f81d4fae-7dec-11d0-a765-00a0c91e6bf6": false }
        });
        let c: ContactCard = serde_json::from_value(raw).unwrap();
        let mems = c.members.as_ref().unwrap();
        assert_eq!(
            mems.get("urn:uuid:f81d4fae-7dec-11d0-a765-00a0c91e6bf6")
                .copied(),
            Some(false),
        );
    }

    /// `keywords` with `false` deserialises cleanly; the type does not
    /// enforce MUST-be-true. Oracle: hand-built JSON.
    #[test]
    fn contact_card_keywords_accepts_false_deserialise() {
        let raw = json!({
            "id": "card-1",
            "keywords": { "IETF": false }
        });
        let c: ContactCard = serde_json::from_value(raw).unwrap();
        let kws = c.keywords.as_ref().unwrap();
        assert_eq!(kws.get("IETF").copied(), Some(false));
    }

    // ── Negative deserialize at the wire boundary (JMAP-glx8.23) ─────────
    //
    // The crate sits at the JMAP-wire deserialize boundary. Downstream
    // consumers (jmap-contacts-server, third-party kits) need a documented
    // contract for what malformed / oversized / spec-violating input does
    // when it reaches `serde_json::from_str` on these types. The tests
    // below pin that contract: the type-crate either accepts cleanly
    // (handler layer rejects later) or returns `Err`. No test asserts a
    // handler-layer behavior; those concerns live in `jmap-contacts-server`
    // (bd:JMAP-yfpq).
    //
    // Oracle: hand-built JSON whose shape is independent of the code
    // under test, per workspace test-integrity rule.

    /// Malformed JSON returns `Err` from `serde_json::from_str` — does not
    /// panic, does not silently parse a partial value. Oracle: three
    /// hand-built malformed inputs.
    #[test]
    fn malformed_json_returns_error() {
        // Truncated object — closing brace missing.
        let truncated = r#"{"id": "card-1""#;
        assert!(serde_json::from_str::<ContactCard>(truncated).is_err());

        // Mismatched braces — closes with `]` instead of `}`.
        let mismatched = r#"{"id": "card-1"]"#;
        assert!(serde_json::from_str::<ContactCard>(mismatched).is_err());

        // Unterminated string literal.
        let unterminated = r#"{"id": "card-1}"#;
        assert!(serde_json::from_str::<ContactCard>(unterminated).is_err());
    }

    /// Oversized `name` value on `AddressBook` deserialises cleanly: the
    /// type accepts up to `String::MAX_LEN` regardless of the 255-octet
    /// upper bound declared by RFC 9610 §2. The handler layer
    /// (`jmap-contacts-server`, bd:JMAP-yfpq) is responsible for
    /// rejecting names that exceed the spec bound with
    /// `invalidProperties` per RFC 8620 §5.3. Oracle: hand-built JSON
    /// with a 1000-byte string.
    #[test]
    fn oversized_name_deserialises_to_typed_field() {
        let big_name = "x".repeat(1000);
        let raw = json!({
            "id": "ab-big-name",
            "name": big_name,
            "description": null,
            "sortOrder": 0,
            "isDefault": false,
            "isSubscribed": true,
            "shareWith": null,
            "myRights": {
                "mayRead": true, "mayWrite": false,
                "mayShare": false, "mayDelete": false
            }
        });
        let ab: AddressBook = serde_json::from_value(raw).unwrap();
        assert_eq!(ab.name.len(), 1000);
    }

    /// `sortOrder` values in `[2^31, 2^32)` deserialise cleanly into the
    /// typed `u32` field, even though RFC 9610 §2 declares the spec range
    /// `[0, 2^31)`. The handler layer is responsible for rejecting values
    /// outside the spec range. This test documents the spec-range vs
    /// type-range gap. Oracle: hand-built JSON with `sortOrder` =
    /// 2_147_483_648 (one past the spec ceiling, well within `u32::MAX`).
    #[test]
    fn sort_order_overflow_outside_spec_range_accepts() {
        let raw = json!({
            "id": "ab-overflow",
            "name": "Overflow",
            "description": null,
            "sortOrder": 2_147_483_648u64,
            "isDefault": false,
            "isSubscribed": true,
            "shareWith": null,
            "myRights": {
                "mayRead": true, "mayWrite": false,
                "mayShare": false, "mayDelete": false
            }
        });
        let ab: AddressBook = serde_json::from_value(raw).unwrap();
        assert_eq!(ab.sort_order, 2_147_483_648);
    }

    /// A Sloppy-Value field (`ContactCard.name`, typed as
    /// `Option<serde_json::Value>`) accepts any JSON shape, including
    /// non-object values like a bare string. The handler layer is
    /// responsible for rejecting non-object wire shapes per RFC 9553 §2.2.
    /// Typed access via `serde_json::from_value::<Name>(...)` returns
    /// `Err` when the wire shape is not an object. Oracle: hand-built
    /// JSON with `name` as a string instead of an object.
    #[test]
    fn sloppy_field_accepts_non_object_value() {
        let raw = json!({
            "id": "card-1",
            "name": "vincent"
        });
        let c: ContactCard = serde_json::from_value(raw).unwrap();
        let name_value = c.name.as_ref().unwrap();
        assert!(name_value.is_string(), "sloppy field preserved the string");

        // Typed access through jmap-jscontact-types fails cleanly — does
        // not panic — when the wire shape is not the expected object.
        let typed = serde_json::from_value::<Name>(name_value.clone());
        assert!(typed.is_err(), "Name expects an object, got a string");
    }

    /// A deeply nested but bounded sloppy-field value deserialises
    /// cleanly: serde_json's default recursion limit is 128 levels of
    /// nesting, and 100-deep nesting fits comfortably under that. Oracle:
    /// hand-built 100-level nested JSON object inside the `name` field.
    #[test]
    fn deeply_nested_sloppy_field_within_serde_json_limit_succeeds() {
        // Build {"id":"c","name":{"x":{"x":...{"x":1}...}}} with 100
        // levels of "x" nesting inside name.
        let mut nested = String::from("1");
        for _ in 0..100 {
            nested = format!("{{\"x\":{nested}}}");
        }
        let payload = format!("{{\"id\":\"c\",\"name\":{nested}}}");
        let c: ContactCard = serde_json::from_str(&payload)
            .expect("100-deep nesting must be within serde_json's default 128 recursion limit");
        assert!(c.name.is_some());
    }

    /// A sloppy-field value nested past serde_json's default 128-level
    /// recursion limit returns `Err`. Oracle: hand-built 200-level
    /// nested JSON object inside the `name` field. This documents that
    /// the type-crate inherits serde_json's stack-overflow defense; it
    /// does NOT make a memory-budget claim — see JMAP-glx8.24 for the
    /// memory-and-CPU contract.
    #[test]
    fn deeply_nested_sloppy_field_beyond_serde_json_limit_errors() {
        let mut nested = String::from("1");
        for _ in 0..200 {
            nested = format!("{{\"x\":{nested}}}");
        }
        let payload = format!("{{\"id\":\"c\",\"name\":{nested}}}");
        let result: Result<ContactCard, _> = serde_json::from_str(&payload);
        assert!(
            result.is_err(),
            "200-deep nesting must exceed serde_json's default 128 recursion limit"
        );
    }

    /// JSON object with a duplicate **typed** field key: `serde_derive`'s
    /// generated visitor REJECTS the duplicate with a "duplicate field"
    /// error. RFC 8259 §4 leaves duplicate-key handling
    /// implementation-defined; this crate's typed-struct contract is
    /// strict-reject. Cross-implementation behavior (Go, Python, Java)
    /// varies — see the extras-collision contract on
    /// [`ContactCard::extra`] (card.rs:247-267). Oracle: hand-built
    /// JSON with two `"uid"` keys.
    #[test]
    fn duplicate_typed_field_keys_rejected() {
        let raw = r#"{"id":"card-1","uid":"first","uid":"second"}"#;
        let err = serde_json::from_str::<ContactCard>(raw)
            .expect_err("typed field duplicate must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("duplicate field"),
            "expected duplicate-field error, got: {msg}"
        );
    }

    /// JSON object with a duplicate **vendor-extra** key: the
    /// `#[serde(flatten)]` `extra` field is backed by
    /// `serde_json::Map<String, Value>`, which last-wins on duplicate
    /// keys (matching the untyped `serde_json::Value` path). This
    /// documents the asymmetry — typed fields reject duplicates,
    /// vendor extras absorb the last value silently. Cross-reference:
    /// [`ContactCard::extra`] collision contract. Oracle: hand-built
    /// JSON with two `"acmeCorpExternalId"` keys.
    #[test]
    fn duplicate_vendor_extra_keys_last_wins() {
        let raw = r#"{"id":"card-1","acmeCorpExternalId":"first","acmeCorpExternalId":"second"}"#;
        let c: ContactCard = serde_json::from_str(raw)
            .expect("vendor-extra duplicates fold into extra map, last-wins");
        assert_eq!(
            c.extra.get("acmeCorpExternalId").and_then(|v| v.as_str()),
            Some("second"),
            "extra map last-wins on duplicate keys"
        );
    }

    // ── validate_extras (JMAP-glx8.25) ───────────────────────────────────
    //
    // The `validate_extras()` method on each affected type is a defensive
    // pre-serialize check that detects keys in `extra` that shadow typed
    // wire-format fields. The tests below pin the contract: clean extras
    // pass, every typed wire-field name collides when programmatically
    // inserted into `extra`, and multi-collision reports list all keys.

    /// `ContactCard::validate_extras` returns `Ok(())` when `extra` is
    /// empty or contains only non-colliding vendor keys.
    #[test]
    fn contact_card_validate_extras_ok_on_clean() {
        let c = ContactCard::default();
        assert!(c.validate_extras().is_ok(), "empty extras must pass");

        let mut c = ContactCard::default();
        c.extra.insert("acmeCorpFoo".into(), json!("bar"));
        assert!(c.validate_extras().is_ok(), "vendor-only key must pass");
    }

    /// `ContactCard::validate_extras` returns `Err` listing every
    /// typed wire-format field name when programmatically inserted
    /// into `extra`. Oracle: the spec'd list of camelCase field names
    /// from RFC 9610 §3 + RFC 9553 §2 + workspace extras-preservation
    /// policy.
    #[test]
    fn contact_card_validate_extras_detects_every_typed_field_collision() {
        // Every typed wire-format field name on ContactCard. Kept here
        // independent of the const in card.rs as the test oracle.
        let typed_fields = [
            "id",
            "addressBookIds",
            "version",
            "created",
            "kind",
            "language",
            "members",
            "prodId",
            "relatedTo",
            "uid",
            "updated",
            "name",
            "nicknames",
            "organizations",
            "speakToAs",
            "titles",
            "emails",
            "onlineServices",
            "phones",
            "preferredLanguages",
            "calendars",
            "schedulingAddresses",
            "addresses",
            "cryptoKeys",
            "directories",
            "links",
            "media",
            "localizations",
            "anniversaries",
            "keywords",
            "notes",
            "personalInfo",
        ];
        for field in typed_fields {
            let mut c = ContactCard::default();
            c.extra.insert(field.into(), json!("collision"));
            let err = c
                .validate_extras()
                .expect_err(&format!("{field} collision must be detected"));
            assert_eq!(err.keys(), &[field.to_string()]);
        }
    }

    /// `ContactCard::validate_extras` reports multiple collisions in
    /// alphabetical order in a single `Err`.
    #[test]
    fn contact_card_validate_extras_reports_multiple_collisions() {
        let mut c = ContactCard::default();
        c.extra.insert("uid".into(), json!("a"));
        c.extra.insert("id".into(), json!("b"));
        c.extra.insert("acmeCorpFoo".into(), json!("c"));
        let err = c.validate_extras().expect_err("multi-collision must error");
        assert_eq!(err.keys(), &["id".to_string(), "uid".to_string()]);
        let msg = err.to_string();
        assert!(msg.contains("id, uid"), "got: {msg}");
    }

    /// `AddressBook::validate_extras` returns `Ok(())` on clean extras
    /// and `Err` on every typed wire-format field collision.
    #[test]
    fn address_book_validate_extras_detects_every_typed_field_collision() {
        let typed_fields = [
            "id",
            "name",
            "description",
            "sortOrder",
            "isDefault",
            "isSubscribed",
            "shareWith",
            "myRights",
        ];
        // Build a minimal AddressBook (without using the literal field
        // names that would interfere with the test).
        let template = json!({
            "id": "ab-test",
            "name": "Test",
            "description": null,
            "sortOrder": 0,
            "isDefault": false,
            "isSubscribed": true,
            "shareWith": null,
            "myRights": {
                "mayRead": true, "mayWrite": false,
                "mayShare": false, "mayDelete": false
            }
        });
        let mut ab: AddressBook = serde_json::from_value(template).unwrap();
        assert!(ab.validate_extras().is_ok(), "clean extras pass");

        for field in typed_fields {
            ab.extra.clear();
            ab.extra.insert(field.into(), json!("collision"));
            let err = ab
                .validate_extras()
                .expect_err(&format!("{field} collision must be detected"));
            assert_eq!(err.keys(), &[field.to_string()]);
        }
    }

    /// `AddressBookRights::validate_extras` returns `Ok(())` on clean
    /// extras and `Err` on every typed wire-format field collision.
    #[test]
    fn address_book_rights_validate_extras_detects_every_typed_field_collision() {
        let typed_fields = ["mayRead", "mayWrite", "mayShare", "mayDelete"];
        let mut r = AddressBookRights::default();
        assert!(r.validate_extras().is_ok(), "clean extras pass");

        for field in typed_fields {
            r.extra.clear();
            r.extra.insert(field.into(), json!(true));
            let err = r
                .validate_extras()
                .expect_err(&format!("{field} collision must be detected"));
            assert_eq!(err.keys(), &[field.to_string()]);
        }
    }
}
