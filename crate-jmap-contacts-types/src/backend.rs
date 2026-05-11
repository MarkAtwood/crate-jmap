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

impl SetObject for crate::AddressBook {
    type Patch = PatchObject;
}

// AddressBook does NOT implement QueryObject — spec has no AddressBook/query.

impl JmapObject for crate::ContactCard {
    const TYPE_NAME: &'static str = "ContactCard";
    type Property = ContactCardProperty;
}

impl GetObject for crate::ContactCard {}

impl SetObject for crate::ContactCard {
    type Patch = PatchObject;
}

impl QueryObject for crate::ContactCard {
    type Filter = crate::ContactCardFilterCondition;
    type Comparator = crate::ContactCardComparator;
}
