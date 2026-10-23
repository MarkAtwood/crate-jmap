//! Property selector enums and [`jmap_types::JmapObject`] impls for JMAP
//! Contacts types.
//!
//! These are defined here so that `jmap-contacts-server` can use them without
//! violating the orphan rule (`JmapObject` is foreign but the contacts types
//! are local to this crate).
//!
//! `AddressBook` does **not** implement [`QueryObject`] because
//! RFC 9610 does not define `AddressBook/query` or
//! `AddressBook/queryChanges`.

use jmap_types::{GetObject, JmapObject, PatchObject, QueryObject, SetObject};

// ---------------------------------------------------------------------------
// Property selector enums (server-side; no serde required)
// ---------------------------------------------------------------------------

/// Property selector for [`crate::AddressBook`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AddressBookProperty {
    /// The `id` property (RFC 9610 §2).
    Id,
    /// The `name` property (RFC 9610 §2).
    Name,
    /// The `description` property (RFC 9610 §2).
    Description,
    /// The `sortOrder` property (RFC 9610 §2).
    SortOrder,
    /// The `isDefault` property (RFC 9610 §2).
    IsDefault,
    /// The `isSubscribed` property (RFC 9610 §2).
    IsSubscribed,
    /// The `shareWith` property (RFC 9610 §2).
    ShareWith,
    /// The `myRights` property (RFC 9610 §2).
    MyRights,
}

/// Property selector for [`crate::ContactCard`] `/get`, `/set`, and `/query`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ContactCardProperty {
    /// The `id` property (RFC 9610 §3; JMAP addition).
    Id,
    /// The `addressBookIds` property (RFC 9610 §3; JMAP addition).
    AddressBookIds,
    /// The `version` property (RFC 9553 §2.1 Metadata).
    Version,
    /// The `created` property (RFC 9553 §2.1 Metadata).
    Created,
    /// The `kind` property (RFC 9553 §2.1 Metadata).
    Kind,
    /// The `language` property (RFC 9553 §2.1 Metadata).
    Language,
    /// The `members` property (RFC 9553 §2.1 Metadata).
    Members,
    /// The `prodId` property (RFC 9553 §2.1 Metadata).
    ProdId,
    /// The `relatedTo` property (RFC 9553 §2.1 Metadata).
    RelatedTo,
    /// The `uid` property (RFC 9553 §2.1 Metadata).
    Uid,
    /// The `updated` property (RFC 9553 §2.1 Metadata).
    Updated,
    /// The `name` property (RFC 9553 §2.2 Name and Organization).
    Name,
    /// The `nicknames` property (RFC 9553 §2.2 Name and Organization).
    Nicknames,
    /// The `organizations` property (RFC 9553 §2.2 Name and Organization).
    Organizations,
    /// The `speakToAs` property (RFC 9553 §2.2 Name and Organization).
    SpeakToAs,
    /// The `titles` property (RFC 9553 §2.2 Name and Organization).
    Titles,
    /// The `emails` property (RFC 9553 §2.3 Contact).
    Emails,
    /// The `onlineServices` property (RFC 9553 §2.3 Contact).
    OnlineServices,
    /// The `phones` property (RFC 9553 §2.3 Contact).
    Phones,
    /// The `preferredLanguages` property (RFC 9553 §2.3 Contact).
    PreferredLanguages,
    /// The `calendars` property (RFC 9553 §2.4 Calendaring).
    Calendars,
    /// The `schedulingAddresses` property (RFC 9553 §2.4 Calendaring).
    SchedulingAddresses,
    /// The `addresses` property (RFC 9553 §2.5 Address).
    Addresses,
    /// The `cryptoKeys` property (RFC 9553 §2.6 Resources).
    CryptoKeys,
    /// The `directories` property (RFC 9553 §2.6 Resources).
    Directories,
    /// The `links` property (RFC 9553 §2.6 Resources).
    Links,
    /// The `media` property (RFC 9553 §2.6 Resources).
    Media,
    /// The `localizations` property (RFC 9553 §2.7 Multilingual).
    Localizations,
    /// The `anniversaries` property (RFC 9553 §2.8 Additional).
    Anniversaries,
    /// The `keywords` property (RFC 9553 §2.8 Additional).
    Keywords,
    /// The `notes` property (RFC 9553 §2.8 Additional).
    Notes,
    /// The `personalInfo` property (RFC 9553 §2.8 Additional).
    PersonalInfo,
}

// ---------------------------------------------------------------------------
// JmapObject impls
// ---------------------------------------------------------------------------

impl JmapObject for crate::AddressBook {
    const TYPE_NAME: &'static str = "AddressBook";
    type Property = AddressBookProperty;
}

impl GetObject for crate::AddressBook {}

/// RFC 9610 §2 (rfc9610.txt:167, 191, 221): `id` is "(immutable;
/// server-set)"; `isDefault` and `myRights` are "(server-set)".
impl SetObject for crate::AddressBook {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &["id", "isDefault", "myRights"];
    const CLIENT_SETTABLE: &'static [&'static str] = &[
        "name",
        "description",
        "sortOrder",
        "isSubscribed",
        "shareWith",
    ];
    const IMMUTABLE: &'static [&'static str] = &[];
}

// AddressBook does NOT implement QueryObject — spec has no AddressBook/query.

impl JmapObject for crate::ContactCard {
    const TYPE_NAME: &'static str = "ContactCard";
    type Property = ContactCardProperty;
}

impl GetObject for crate::ContactCard {}

/// RFC 9610 §3 (rfc9610.txt:305): `id` is "(immutable; server-set)".
/// `addressBookIds` and the RFC 9553 Card properties carry neither
/// attribute.
impl SetObject for crate::ContactCard {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &["id"];
    const CLIENT_SETTABLE: &'static [&'static str] = &[
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
    const IMMUTABLE: &'static [&'static str] = &[];
}

impl QueryObject for crate::ContactCard {
    type Filter = crate::ContactCardFilter;
    type Comparator = crate::ContactCardComparator;
}

#[cfg(test)]
mod tests {
    //! Each test builds a fully populated object with a struct literal, so a
    //! new field is a compile error here until it is set, then a
    //! classification failure until the `SetObject` lists name it.

    use jmap_types::{check_property_classification, Id, PatchObject, UTCDate};
    use serde_json::{json, Map, Value};
    use std::collections::BTreeMap;

    fn de<T: serde::de::DeserializeOwned>(v: Value) -> T {
        serde_json::from_value(v).expect("hand-written fixture deserializes")
    }

    fn rights() -> crate::AddressBookRights {
        de(json!({"mayRead": true, "mayWrite": true, "mayShare": true, "mayDelete": true}))
    }

    #[test]
    fn address_book_properties_are_classified() {
        let obj = crate::AddressBook {
            id: Id::from("a1"),
            name: "n".into(),
            description: Some("d".into()),
            sort_order: 1,
            is_default: true,
            is_subscribed: true,
            share_with: Some(BTreeMap::from([(Id::from("p1"), rights())])),
            my_rights: rights(),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[test]
    fn contact_card_properties_are_classified() {
        let v = || Some(json!({}));
        let patch: PatchObject = de(json!({"name/full": "N"}));
        let obj = crate::ContactCard {
            id: Some(Id::from("c1")),
            address_book_ids: Some(BTreeMap::from([(Id::from("a1"), true)])),
            version: Some("1.0".into()),
            created: Some(UTCDate::from("2026-01-01T00:00:00Z")),
            kind: Some("individual".into()),
            language: Some("en".into()),
            members: Some(BTreeMap::from([("u2".into(), true)])),
            prod_id: Some("p".into()),
            related_to: v(),
            uid: Some("u".into()),
            updated: Some(UTCDate::from("2026-01-01T00:00:00Z")),
            name: v(),
            nicknames: v(),
            organizations: v(),
            speak_to_as: v(),
            titles: v(),
            emails: v(),
            online_services: v(),
            phones: v(),
            preferred_languages: v(),
            calendars: v(),
            scheduling_addresses: v(),
            addresses: v(),
            crypto_keys: v(),
            directories: v(),
            links: v(),
            media: v(),
            localizations: Some(BTreeMap::from([("de".into(), patch)])),
            anniversaries: v(),
            keywords: Some(BTreeMap::from([("k".into(), true)])),
            notes: v(),
            personal_info: v(),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }
}
