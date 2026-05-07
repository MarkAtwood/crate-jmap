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

use jmap_types::{GetObject, JmapObject, QueryObject, SetObject};

// ---------------------------------------------------------------------------
// Property selector enums (server-side; no serde required)
// ---------------------------------------------------------------------------

/// Property selector for [`crate::AddressBook`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AddressBookProperty {
    Id,
    Name,
    Description,
    SortOrder,
    IsDefault,
    IsSubscribed,
    ShareWith,
    MyRights,
}

/// Property selector for [`crate::ContactCard`] `/get`, `/set`, and `/query`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ContactCardProperty {
    Id,
    AddressBookIds,
    Version,
    Created,
    Kind,
    Language,
    Members,
    ProdId,
    RelatedTo,
    Uid,
    Updated,
    Name,
    Nicknames,
    Organizations,
    SpeakToAs,
    Titles,
    Emails,
    OnlineServices,
    Phones,
    PreferredLanguages,
    Calendars,
    SchedulingAddresses,
    Addresses,
    CryptoKeys,
    Directories,
    Links,
    Media,
    Localizations,
    Anniversaries,
    Keywords,
    Notes,
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
    type Patch = serde_json::Value;
}

// AddressBook does NOT implement QueryObject — spec has no AddressBook/query.

impl JmapObject for crate::ContactCard {
    const TYPE_NAME: &'static str = "ContactCard";
    type Property = ContactCardProperty;
}

impl GetObject for crate::ContactCard {}

impl SetObject for crate::ContactCard {
    type Patch = serde_json::Value;
}

impl QueryObject for crate::ContactCard {
    type Filter = crate::ContactCardFilterCondition;
    type Comparator = crate::ContactCardComparator;
}
