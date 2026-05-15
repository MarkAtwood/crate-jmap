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
| `card.rs` | `ContactCard`, `ContactCardFilterCondition`, `ContactCardComparator` | RFC 9610 §3, §3.3, §3.3.1; RFC 9553 §2 |
| `backend.rs` | `AddressBookProperty`, `ContactCardProperty` | RFC 9610 §2, §3 |
| `capability.rs` | `ContactsCapability`, `ContactsAccountCapability`, `JMAP_CONTACTS_URI` | RFC 9610 §1.4.1 |

The `string_enum!` macro used by `backend.rs` is imported from `jmap-types`
(consolidated under JMAP-wk77); this crate does not define its own copy.

JSContact sub-object types (`Name`, `EmailAddress`, `Phone`, `Address`, etc.)
defined in RFC 9553 §2.x are **not** exported as typed Rust structs. All
JSContact collection fields on `ContactCard` are `Option<serde_json::Value>`.
See "Key Design Decisions" below.

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

### 2. JSContact sub-objects are `serde_json::Value` on the wire; typed views via `jmap-jscontact-types`

RFC 9553 represents collections as `String[ObjectType]` — JSON objects where all
keys are JSContact `Id` values (base64url, 1–255 octets) and all values are the
same sub-object type. This crate's `ContactCard` keeps every JSContact
collection field as `Option<serde_json::Value>` so the wire format is the
anchor and no round-trip information is lost across schema-extension changes.

Typed access to the sub-objects is **opt-in** and lives in the sibling
`jmap-jscontact-types` crate (RFC 9553 typed sub-types). This crate
re-exports those types at the top level (`jmap_contacts_types::Name`,
`jmap_contacts_types::EmailAddress`, etc.) and via a `jscontact` module
alias (`jmap_contacts_types::jscontact::Name`) mirroring the
`jmap_calendars_types::jscalendar` pattern. Callers obtain typed views via:

```rust
use jmap_contacts_types::{ContactCard, Name};
let card: ContactCard = serde_json::from_value(json)?;
let name: Name = serde_json::from_value(card.name.unwrap())?;
```

See `crate-jmap-jscontact-types/PLAN.md` for the typed-sub-object design
rationale, the full list of types, and the hybrid round-trip policy.

### 3. All ContactCard fields are Option

`ContactCard` uses `Option` for almost every field because:
- RFC 9553 makes most fields optional.
- JMAP `properties` argument allows partial responses (clients request only
  what they need).  A field absent from the server response must not fail
  deserialization.

Mandatory RFC 9553 fields (`@type`, `version`, `uid`) are still `Option<String>`
on the Rust struct — mandatory on creation (validated by the server handler), but
a partial `/get` response may omit them.

### 4. @type fields — pass through untouched

RFC 9553 defines `@type` as a discriminator on sub-objects.  Because sub-object
fields on `ContactCard` are `serde_json::Value`, this crate does not interpret
or validate `@type` at the type-crate layer — the field round-trips through
serde untouched. Constraint enforcement (including `@type` discriminator
validation) is the responsibility of the handler layer (`jmap-contacts-server`)
or the caller.

Use `#[serde(skip_serializing_if = "Option::is_none")]` for optional fields on
`ContactCard` and `AddressBook` to keep wire output minimal.

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
JSContact spec.  Because `media` on `ContactCard` is `serde_json::Value`, the
`blobId` key passes through serde untouched. Constraint enforcement and
blob-id resolution are the responsibility of the handler layer.

### 9. ContactCardFilter — FilterCondition fields use slash-separated names

The contacts draft (§3.3.1) defines filter conditions with field names like
`"name/given"`, `"name/surname"`, `"name/surname2"`.  These are JSON keys that
contain forward slashes.  In Rust, they map to struct fields with
`#[serde(rename = "name/given")]` attributes.

### 10. PartialDate and other JSContact value shapes

RFC 9553 defines several value shapes (e.g. `PartialDate` for anniversaries)
that ride inside the JSContact collection fields. The `ContactCard` field
shapes themselves stay as `Option<serde_json::Value>` on the wire so that
unknown-shape round-trip fidelity is preserved.

Typed Rust representations of these value shapes live in the sibling
`jmap-jscontact-types` crate and are re-exported here for caller
ergonomics: `PartialDate`, `Timestamp`, `AnniversaryDate`, `Anniversary`,
`Name`, `Address`, etc. See `crate-jmap-jscontact-types/PLAN.md` for the
full list and the hybrid-design rationale. Semantic validation lives in
the handler or the caller.

**Exception — typed envelope for `localizations`:** the `localizations` field
was promoted from `Option<serde_json::Value>` to
`Option<HashMap<String, jmap_types::PatchObject>>` per JMAP-46s0 (epic
JMAP-trmz, the cross-extension PatchObject typed-envelope sweep). This
matches the canonical shape in `jmap-tasks-types::Task::localizations` and
encodes the RFC 9553 §2.7 contract — `language-tag → PatchObject` — at the
type system level. The wire format is byte-identical to the previous
`Value` shape via `#[serde(transparent)]` on `PatchObject`; only the outer
object-map structure is now enforced at deserialize time. Inner patch
values remain opaque per RFC 8620 §5.3.

### 11. Enum catch-alls — open-ended string enums via `string_enum!`

Several enum-like fields used at the JMAP layer (e.g. `AddressBookProperty`,
`ContactCardProperty`) are open-ended: the spec allows vendor-specific
extensions. The `string_enum!` macro (consolidated to `jmap-types` per
JMAP-wk77 and imported as `use jmap_types::impl_string_enum;`) produces a
non-exhaustive enum with a `Custom(String)` variant that round-trips unknown
values through serde. Closed-set enums are not used here because the spec
does not declare any sub-set non-extensible.

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
    pub address_book_ids: Option<HashMap<Id, bool>>, // wire: "addressBookIds"

    // ── RFC 9553 §2.1 Metadata ───────────────────────────────────────────
    // @type is always "Card" — serialized, not stored as a field
    pub version: Option<String>,                  // mandatory: "1.0"
    pub created: Option<String>,                  // UTCDateTime
    pub kind: Option<String>,                     // "individual"|"group"|"org"|...
    pub language: Option<String>,                 // RFC 5646 language tag
    pub members: Option<HashMap<String, bool>>,   // uid set; for kind="group"
    pub prod_id: Option<String>,
    pub uid: Option<String>,                      // mandatory on creation
    pub updated: Option<String>,                  // UTCDateTime; server-set

    // ── RFC 9553 sub-object fields (all `serde_json::Value`) ───────────
    // The crate does NOT provide typed structs for JSContact sub-objects.
    // Each of the fields below is `Option<serde_json::Value>`; callers
    // deserialize into their own structs using RFC 9553 as the schema.
    pub name: Option<serde_json::Value>,
    pub nicknames: Option<serde_json::Value>,
    pub organizations: Option<serde_json::Value>,
    pub speak_to_as: Option<serde_json::Value>,
    pub titles: Option<serde_json::Value>,
    pub emails: Option<serde_json::Value>,
    pub online_services: Option<serde_json::Value>,
    pub phones: Option<serde_json::Value>,
    pub preferred_languages: Option<serde_json::Value>,
    pub calendars: Option<serde_json::Value>,
    pub scheduling_addresses: Option<serde_json::Value>,
    pub addresses: Option<serde_json::Value>,
    pub crypto_keys: Option<serde_json::Value>,
    pub directories: Option<serde_json::Value>,
    pub links: Option<serde_json::Value>,
    pub media: Option<serde_json::Value>,
    pub localizations: Option<HashMap<String, jmap_types::PatchObject>>,
    pub anniversaries: Option<serde_json::Value>,
    pub keywords: Option<HashMap<String, bool>>,
    pub notes: Option<serde_json::Value>,
    pub personal_info: Option<serde_json::Value>,
    pub related_to: Option<serde_json::Value>,
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
  lib.rs           re-exports; #[forbid(unsafe_code)]
  addressbook.rs   AddressBook, AddressBookRights
  card.rs          ContactCard, ContactCardFilterCondition, ContactCardComparator
  backend.rs       AddressBookProperty, ContactCardProperty
  capability.rs    ContactsCapability, ContactsAccountCapability,
                   JMAP_CONTACTS_URI const
```

The `string_enum!` macro is imported from `jmap-types` (see JMAP-wk77);
this crate does not define its own copy.

There is no `jscontact/` submodule inside this crate. Typed Rust structs
for JSContact sub-objects (RFC 9553 §2.x) live in the sibling
`jmap-jscontact-types` crate and are re-exported at the top level and via
the `jscontact` module alias (`pub use jmap_jscontact_types as jscontact`).
See "Key Design Decisions" §2 for the rationale.

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
  (asserted as `serde_json::Value` shapes, not typed sub-object structs)
- `ContactCard` with `kind: "group"` and `members` map
- `ContactCardFilterCondition` with slash-keyed fields (`name/given`)
- `AddressBookRights` — four fields only, no extras

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
jmap-types           = { path = "../crate-jmap-types" }
jmap-jscontact-types = { path = "../crate-jmap-jscontact-types" }
serde                = { version = "1", features = ["derive"] }
serde_json           = "1"
```

No tokio, no async, no network deps.

## Type-design constraints

### Extras-preservation policy (JMAP-lbdy)

Every public `Deserialize` struct that appears on the JMAP wire carries an
`extra` field per the workspace extras-preservation policy (see workspace
`AGENTS.md`):

```rust
#[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
pub extra: serde_json::Map<String, serde_json::Value>,
```

In scope in this crate (each has a round-trip preservation test):

- `ContactCard` (card.rs).
- `AddressBookRights`, `AddressBook` (addressbook.rs).

The 23 typed sub-types in the re-exported `jscontact` module
(`jmap-jscontact-types`) — `Name`, `Nickname`, `Organization`,
`OrgUnit`, `SpeakToAs`, `Pronouns`, `Title`, `EmailAddress`,
`OnlineService`, `Phone`, `LanguagePref`, `Calendar`,
`SchedulingAddress`, `Address`, `CryptoKey`, `Directory`, `Link`,
`Media`, `Anniversary`, `Note`, `Author`, `PersonalInfo`, `Relation` —
also carry `extra` fields by virtue of the policy applied in their
home crate.

Out of scope:

- `ContactCardFilterCondition`, `ContactCardComparator` — filter and
  comparator algebra, per workspace policy. The existing rustdoc on
  each already documents the exclusion.
- `ContactsCapability`, `ContactsAccountCapability` — capability
  objects.
- `NameComponent`, `AddressComponent`, `PartialDate`, `Timestamp` —
  Hash-derived sub-types in `jmap-jscontact-types`; pending Hash-vs-
  extras decision in JMAP-lbdy.12.

### New-type rule

Any new public `Deserialize` struct added to this crate that appears on
the JMAP wire MUST include the `extra` field from day one with the
documented serde attributes and at least one round-trip preservation
test.

## JMAP Object Metadata `relatedType` declarations

The JMAP Object Metadata extension
([draft-ietf-jmap-metadata-01](https://datatracker.ietf.org/doc/draft-ietf-jmap-metadata/))
defines a companion `Metadata` object keyed by `(relatedType, relatedId)`
that attaches vendor-defined `Annotation`s — and, for some types, WebDAV
`WebDavMetadata` records — to objects defined elsewhere in the workspace.

The data types in this crate that are valid `relatedType` values:

| relatedType | Flavours supported by spec |
|---|---|
| `AddressBook` | `Annotation`; `WebDavMetadata` (draft §2.1.3 SHOULD) |
| `ContactCard` | `Annotation`; `WebDavMetadata` (draft §2.1.3 SHOULD) |

Servers that declare `urn:ietf:params:jmap:metadata` MAY restrict the
set of supported `relatedType`s via the capability's `dataTypes`
property.

Implementation crates: `jmap-metadata-types`, `jmap-metadata-server`,
`jmap-metadata-client` (bd JMAP-06zp).
