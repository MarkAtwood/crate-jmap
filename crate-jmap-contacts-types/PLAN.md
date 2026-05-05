# jmap-contacts-types — Implementation Plan

JMAP Contacts data types: AddressBook and ContactCard (JSContact).  Types only —
no method handlers, no async, no network I/O.  This crate sits between
`jmap-types` (shared JMAP base primitives) and the server/client crates that
consume these types.

## Crate Family Position

```
jmap-types (RFC 8620 wire primitives)
    └── jmap-contacts-types  ← this crate
            ├── jmap-contacts-server (method handlers)
            └── jmap-contacts-client (client extension trait)
```

## What This Crate Covers

Two JMAP data types are defined by RFC 9610.  The ContactCard
type is a JMAP binding of the JSContact Card format defined in RFC 9553.

| Module | Type(s) | Source |
|---|---|---|
| `addressbook.rs` | `AddressBook`, `AddressBookRights` | RFC 9610 §2 |
| `card.rs` | `ContactCard` (the JMAP-wrapped JSContact Card) | RFC 9610 §3, RFC 9553 §2 |
| `jscontact/name.rs` | `Name`, `NameComponent`, `NameComponentKind` | RFC 9553 §2.2.1 |
| `jscontact/nickname.rs` | `Nickname` | RFC 9553 §2.2.2 |
| `jscontact/org.rs` | `Organization`, `OrgUnit` | RFC 9553 §2.2.3 |
| `jscontact/speak_to.rs` | `SpeakToAs`, `Pronouns`, `GrammaticalGender` | RFC 9553 §2.2.4 |
| `jscontact/title.rs` | `Title`, `TitleKind` | RFC 9553 §2.2.5 |
| `jscontact/email.rs` | `EmailAddress` | RFC 9553 §2.3.1 |
| `jscontact/online.rs` | `OnlineService` | RFC 9553 §2.3.2 |
| `jscontact/phone.rs` | `Phone`, `PhoneFeature` | RFC 9553 §2.3.3 |
| `jscontact/lang.rs` | `LanguagePref` | RFC 9553 §2.3.4 |
| `jscontact/address.rs` | `Address`, `AddressComponent`, `AddressComponentKind` | RFC 9553 §2.5.1 |
| `jscontact/resource.rs` | `CryptoKey`, `Directory`, `Link`, `Media` | RFC 9553 §2.6 |
| `jscontact/calendar.rs` | `Calendar`, `SchedulingAddress` | RFC 9553 §2.4 |
| `jscontact/anniversary.rs` | `Anniversary`, `AnniversaryKind`, `PartialDate` | RFC 9553 §2.8.1 |
| `jscontact/note.rs` | `Note` | RFC 9553 §2.8.3 |
| `jscontact/personal.rs` | `PersonalInfo`, `PersonalInfoKind` | RFC 9553 §2.8.4 |
| `jscontact/relation.rs` | `Relation` | RFC 9553 §2.1.8 |
| `jscontact/localization.rs` | `Localization` (PatchObject map) | RFC 9553 §2.7.1 |
| `query.rs` | `ContactCardFilter`, `ContactCardFilterCondition`, `ContactCardComparator` | RFC 9610 §3.3 |

## What Is Out of Scope

- Method handlers (`AddressBook/get`, `ContactCard/set`, etc.) — those live in
  `jmap-contacts-server`
- vCard/jCard import/export — this crate holds the JSON wire types only
- JSContact-to-vCard conversion (RFC 9555) — out of scope for this project
- Transport and network I/O — no tokio, no reqwest
- PATCH application semantics — `jmap-contacts-server` applies patches; this
  crate holds the types

## Key Design Decisions

### 1. ContactCard is the JMAP object name — not "Contact" or "Card"

The JMAP draft (RFC 9610) registers the data type as
`ContactCard`.  The JMAP method names are `ContactCard/get`, `ContactCard/set`,
etc.  The §4.1 example in the draft that shows `"Contact/get"` is a typo in the
draft; all normative sections use `ContactCard`.

The Rust struct is named `ContactCard`.  It embeds all RFC 9553 Card fields
directly (flattened), plus the JMAP-specific `id` and `addressBookIds` fields
added by the contacts draft.

### 2. JSContact object types use Id-keyed maps

RFC 9553 represents collections as `String[ObjectType]` — JSON objects where all
keys are JSContact `Id` values (base64url, 1–255 octets) and all values are the
same sub-object type.  In Rust these are `HashMap<String, T>`.  The key type is
`String` rather than `jmap_types::Id` because JSContact Ids and JMAP Ids have
the same character constraints but are defined independently.

### 3. All ContactCard fields are Option

`ContactCard` uses `Option` for almost every field because:
- RFC 9553 makes most fields optional.
- JMAP `properties` argument allows partial responses (clients request only
  what they need).  A field absent from the server response must not fail
  deserialization.

Mandatory RFC 9553 fields (`@type`, `version`, `uid`) are still `Option<String>`
on the Rust struct — mandatory on creation (validated by the server handler), but
a partial `/get` response may omit them.

### 4. @type fields — serialize but ignore on deserialize

RFC 9553 defines `@type` as a discriminator on sub-objects.  Many sub-object
`@type` values are implied by context (e.g., an object in the `emails` map is
always an `EmailAddress`).  Strategy:

- Serialize `@type` with the correct literal value using `#[serde(rename = "@type")]`.
- On deserialize, accept and ignore `@type` — do not validate it at the
  deserialization layer.  The handler layer validates if needed.
- Use `#[serde(skip_serializing_if = "Option::is_none")]` for optional fields to
  keep wire output minimal.

### 5. ContactCard.kind and groups

The draft (§3, referencing RFC 9553 §2.1.6) specifies that a `ContactCard` with
`kind: "group"` represents a group of contacts, with the `members` property
holding a `String[Boolean]` set of UIDs.  There is no separate `ContactGroup`
JMAP type.  Groups are just `ContactCard` objects distinguished by `kind`.

### 6. addressBookIds wire format

`addressBookIds: Id[Boolean]` is serialized as a JSON object where each key is an
AddressBook `Id` and each value is `true`.  In Rust: `HashMap<String, bool>`.
The serde default handles this correctly.

### 7. AddressBookRights — no mayAdmin, no mayCreateChild

The draft (§2) defines exactly four rights: `mayRead`, `mayWrite`, `mayShare`,
`mayDelete`.  The thin PLAN.md listed `mayAdmin` and `mayCreateChild` — those
are not in the spec and MUST NOT be included.

### 8. Media blobId — extra JMAP property on Media sub-objects

The contacts draft (§3) adds a `blobId: Id` property to any `Media` object
within a ContactCard (RFC 9553 §2.6.4).  This is a JMAP-only extension to the
JSContact spec.  The `Media` struct includes `blob_id: Option<Id>` serialized as
`"blobId"`.

### 9. ContactCardFilter — FilterCondition fields use slash-separated names

The contacts draft (§3.3.1) defines filter conditions with field names like
`"name/given"`, `"name/surname"`, `"name/surname2"`.  These are JSON keys that
contain forward slashes.  In Rust, they map to struct fields with
`#[serde(rename = "name/given")]` attributes.

### 10. PartialDate for anniversaries

RFC 9553 §2.8.1 uses a `PartialDate` type for anniversary dates — a date that
may have the year, month, or day omitted.  Represent as a struct with
`Option<u16>` year, `Option<u8>` month, `Option<u8>` day plus a `@type`
discriminator (`"PartialDate"` vs `"Timestamp"`).  Use custom serde to handle
the two variants.

### 11. Localization — PatchObject semantics

The `localizations` property (RFC 9553 §2.7.1) maps language tags to
`PatchObject` values — JSON objects of `String[*]` (JSON Pointer path → value).
Represent as `HashMap<String, HashMap<String, serde_json::Value>>`.  This crate
does not interpret the patches; server/client code does.

### 12. Enum catch-alls — String not enum

Many JSContact string-enum values (kind, context names, phone features) allow
vendor-specific extensions.  These MUST NOT be represented as Rust enums with
`#[serde(deny_unknown_fields)]`.  Strategy: use `String` for all open-ended
enum fields (e.g., `CardKind = String`, `PhoneFeature = String`) with named
constants in a `mod consts` submodule.  Use actual Rust enums only for closed
sets that the spec declares non-extensible (none exist in this spec).

## AddressBook Type (RFC 9610 §2)

> **Note:** `totalContacts` is NOT a field in RFC 9610.  Do not
> add it.  The spec defines no such property on AddressBook.  Any prior audit
> finding referring to `totalContacts` was incorrect.

```rust
pub struct AddressBook {
    pub id: Id,                                   // immutable; server-set
    pub name: String,                             // max 255 UTF-8 octets; non-empty
    pub description: Option<String>,              // default: null
    pub sort_order: u32,                          // default: 0; range [0, 2^31)
    pub is_default: bool,                         // server-set
    pub is_subscribed: bool,
    pub share_with: Option<HashMap<Id, AddressBookRights>>, // null = not shared
    pub my_rights: AddressBookRights,             // server-set
}

pub struct AddressBookRights {
    pub may_read: bool,
    pub may_write: bool,
    pub may_share: bool,
    pub may_delete: bool,
}
```

## ContactCard Type (RFC 9610 §3, RFC 9553 §2)

The JMAP `ContactCard` embeds RFC 9553 `Card` fields.  Fields are grouped by
RFC 9553 section:

```rust
pub struct ContactCard {
    // ── JMAP additions (RFC 9610 §3) ─────────────────────────────────
    pub id: Option<Id>,                           // immutable; server-set
    pub address_book_ids: Option<HashMap<String, bool>>, // wire: "addressBookIds"

    // ── RFC 9553 §2.1 Metadata ───────────────────────────────────────────
    // @type is always "Card" — serialized, not stored as a field
    pub version: Option<String>,                  // mandatory: "1.0"
    pub created: Option<String>,                  // UTCDateTime
    pub kind: Option<String>,                     // "individual"|"group"|"org"|...
    pub language: Option<String>,                 // RFC 5646 language tag
    pub members: Option<HashMap<String, bool>>,   // uid set; for kind="group"
    pub prod_id: Option<String>,
    pub related_to: Option<HashMap<String, Relation>>,
    pub uid: Option<String>,                      // mandatory on creation
    pub updated: Option<String>,                  // UTCDateTime; server-set

    // ── RFC 9553 §2.2 Name and Organization ─────────────────────────────
    pub name: Option<Name>,
    pub nicknames: Option<HashMap<String, Nickname>>,
    pub organizations: Option<HashMap<String, Organization>>,
    pub speak_to_as: Option<SpeakToAs>,
    pub titles: Option<HashMap<String, Title>>,

    // ── RFC 9553 §2.3 Contact ────────────────────────────────────────────
    pub emails: Option<HashMap<String, EmailAddress>>,
    pub online_services: Option<HashMap<String, OnlineService>>,
    pub phones: Option<HashMap<String, Phone>>,
    pub preferred_languages: Option<HashMap<String, LanguagePref>>,

    // ── RFC 9553 §2.4 Calendaring ────────────────────────────────────────
    pub calendars: Option<HashMap<String, Calendar>>,
    pub scheduling_addresses: Option<HashMap<String, SchedulingAddress>>,

    // ── RFC 9553 §2.5 Address ────────────────────────────────────────────
    pub addresses: Option<HashMap<String, Address>>,

    // ── RFC 9553 §2.6 Resources ──────────────────────────────────────────
    pub crypto_keys: Option<HashMap<String, CryptoKey>>,
    pub directories: Option<HashMap<String, Directory>>,
    pub links: Option<HashMap<String, Link>>,
    pub media: Option<HashMap<String, Media>>,

    // ── RFC 9553 §2.7 Multilingual ───────────────────────────────────────
    pub localizations: Option<HashMap<String, HashMap<String, serde_json::Value>>>,

    // ── RFC 9553 §2.8 Additional ─────────────────────────────────────────
    pub anniversaries: Option<HashMap<String, Anniversary>>,
    pub keywords: Option<HashMap<String, bool>>,
    pub notes: Option<HashMap<String, Note>>,
    pub personal_info: Option<HashMap<String, PersonalInfo>>,
}
```

## ContactCardFilter Type (RFC 9610 §3.3.1)

```rust
pub struct ContactCardFilterCondition {
    pub in_address_book: Option<Id>,
    pub uid: Option<String>,
    pub has_member: Option<String>,
    pub kind: Option<String>,
    pub created_before: Option<String>,           // UTCDate
    pub created_after: Option<String>,
    pub updated_before: Option<String>,
    pub updated_after: Option<String>,
    pub text: Option<String>,
    pub name: Option<String>,
    #[serde(rename = "name/given")]
    pub name_given: Option<String>,
    #[serde(rename = "name/surname")]
    pub name_surname: Option<String>,
    #[serde(rename = "name/surname2")]
    pub name_surname2: Option<String>,
    pub nickname: Option<String>,
    pub organization: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub online_service: Option<String>,
    pub address: Option<String>,
    pub note: Option<String>,
}

pub type ContactCardFilter = jmap_types::query::Filter<ContactCardFilterCondition>;

pub struct ContactCardComparator {
    pub property: String,                         // "created"|"updated"|"name/given"|...
    #[serde(default = "default_true")]
    pub is_ascending: bool,
    pub collation: Option<String>,
}
```

## Module Layout

```
src/
  lib.rs                  re-exports; #[forbid(unsafe_code)]
  addressbook.rs          AddressBook, AddressBookRights
  card.rs                 ContactCard (top-level JMAP object)
  query.rs                ContactCardFilter, ContactCardFilterCondition,
                          ContactCardComparator
  jscontact/
    mod.rs                pub use for all sub-modules
    name.rs               Name, NameComponent, NameComponentKind consts
    nickname.rs           Nickname
    org.rs                Organization, OrgUnit
    speak_to.rs           SpeakToAs, Pronouns, GrammaticalGender consts
    title.rs              Title, TitleKind consts
    email.rs              EmailAddress
    online.rs             OnlineService
    phone.rs              Phone, PhoneFeature consts
    lang.rs               LanguagePref
    address.rs            Address, AddressComponent, AddressComponentKind consts
    resource.rs           CryptoKey, Directory, Link, Media (+ blobId)
    calendar.rs           Calendar, SchedulingAddress
    anniversary.rs        Anniversary, AnniversaryKind consts, PartialDate
    note.rs               Note
    personal.rs           PersonalInfo, PersonalInfoKind consts
    relation.rs           Relation
    localization.rs       type alias for localization map
```

## Test Oracle Strategy

Tests must use independent oracles — never derive expected values from the code
under test.  Acceptable sources:

1. Literal JSON from RFC 9610 examples (§4.1, §4.2) —
   copy-pasted from the draft text.
2. Literal JSON from RFC 9553 examples (Figures 6–39 in §2) — copy-pasted
   from the RFC text.
3. Hand-written JSON constructed directly from the field descriptions in the
   spec.

All tests are `#[test]` (no tokio).  Roundtrip tests (`serialize → deserialize`)
verify serde consistency but are not a substitute for spec-grounded oracle tests.

Key test cases:
- `AddressBook` round-trip using the §4.1 example response
- `ContactCard` with `emails`, `phones`, `name` using the RFC 9553 figures
- `ContactCard` with `kind: "group"` and `members` map
- `ContactCardFilterCondition` with slash-keyed fields (`name/given`)
- `AddressBookRights` — four fields only, no extras
- `Media` with `blobId` serializes the extra JMAP field

## Congruence with jmap-mail-types

| jmap-mail-types | jmap-contacts-types | Notes |
|---|---|---|
| `Mailbox` | `AddressBook` | Container object; similar rights structure |
| `Email` | `ContactCard` | Primary object; both use all-Option fields |
| `EmailFilterCondition` | `ContactCardFilterCondition` | Filter with domain-specific fields |
| `EmailComparator` | `ContactCardComparator` | Comparator with property string |
| `MailboxRole` | `CardKind` (String consts) | Open-ended enum; use String not enum |
| `MailboxRights` | `AddressBookRights` | Closed set of boolean rights |

## Spec References

- `~/PROJECT/jmap-chat-spec/references/RFC 9610.txt` —
  JMAP Contacts (normative for AddressBook, ContactCard JMAP binding, filter,
  capability)
- `~/PROJECT/jmap-chat-spec/references/rfc9553.txt` — RFC 9553 JSContact
  (normative for all Card sub-object types)
- `~/PROJECT/jmap-chat-spec/references/rfc8620.txt` — JMAP base protocol
  (for Filter, Comparator, Id, UTCDate, State)

## Dependencies

```toml
jmap-types = { path = "../crate-jmap-types" }
serde      = { version = "1", features = ["derive"] }
serde_json = "1"
```

No tokio, no async, no network deps.
