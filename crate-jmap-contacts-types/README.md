# jmap-contacts-types

Serde-annotated Rust types for JMAP Contacts ([RFC 9610]) and
JSContact ([RFC 9553]). Types only — no method handlers, no async, no network I/O.

## What it is

| Type | Module | Source |
|---|---|---|
| `AddressBook` | `addressbook` | RFC 9610 §2 |
| `AddressBookRights` | `addressbook` | RFC 9610 §2 |
| `ContactCard` | `card` | RFC 9610 §3, RFC 9553 §2 |
| `ContactCardFilterCondition` | `card` | RFC 9610 §3.3.1 |
| `ContactCardComparator` | `card` | RFC 9610 §3.3 |
| `AddressBookProperty` | `backend` | RFC 9610 §2 |
| `ContactCardProperty` | `backend` | RFC 9610 §3, RFC 9553 §2 |
| `ContactsCapability` | `capability` | RFC 9610 §1.4.1 |
| `ContactsAccountCapability` | `capability` | RFC 9610 §1.4.1 |
| `JMAP_CONTACTS_URI` const | `capability` | RFC 9610 §1.4.1 |

JSContact sub-objects (`Name`, `EmailAddress`, `Phone`, `Address`, etc.)
live in the sibling [`jmap-jscontact-types`] crate and are re-exported
here for caller ergonomics. The sub-object fields on `ContactCard`
themselves remain `Option<serde_json::Value>` as the wire-format anchor;
typed access is opt-in via `serde_json::from_value` into the corresponding
sub-type. See the [JSContact sub-objects](#jscontact-sub-objects) section
below for the usage pattern.

[`jmap-jscontact-types`]: https://crates.io/crates/jmap-jscontact-types

## What it's for

RFC 9610 data types, consumed by
`jmap-contacts-server` (method handlers + the `ContactsBackend` trait)
and `jmap-contacts-client` (typed method bindings). Re-exports
`jmap-jscontact-types` as the `jscontact` module so callers can opt into
typed RFC 9553 JSContact sub-objects without taking an extra direct
dependency. Sibling to `jmap-mail-types` and `jmap-calendars-types` in
the workspace's extension-types family.

## Filter extensibility

Filter and comparator types in this crate — `ContactCardFilterCondition`,
`ContactCardComparator`, and the generic `Filter<T>` / `Operator` re-exported
from `jmap-types` — are **intentionally not extensible** via vendor "extras"
fields. A filter clause the server does not understand silently breaks query
correctness: the client gets the wrong set of records back with no error
signal. So these types deliberately have no `extra` catch-all field.

Vendors who need to filter on custom fields have two options:

- **IETF-track (recommended).** Use the JMAP Object Metadata extension
  (`draft-ietf-jmap-metadata`, capability URI `urn:ietf:params:jmap:metadata`),
  which defines a `Metadata` / `Annotation` companion object keyed by
  `(relatedType, relatedId)` with capability-declared schema (`metadataTypes`
  / `maxDepth`) and a `Metadata/query` `textMatch` filter. This is the
  workspace's recommended path for vendor data that needs to be queryable.
  Implemented in [`jmap-metadata-types`](../crate-jmap-metadata-types),
  [`jmap-metadata-server`](../crate-jmap-metadata-server), and
  [`jmap-metadata-client`](../crate-jmap-metadata-client) (bd JMAP-06zp).
- **Pre-IETF escape.** If you cannot wait for the metadata draft, escape the
  filter tree to `serde_json::Value` or fork the `ContactCardFilterCondition`
  type. See
  [`crate-jmap-calendars-types/PLAN.md`](../crate-jmap-calendars-types/PLAN.md)
  for the hybrid sloppy-value pattern.

This policy is part of the workspace extras-preservation policy documented in
the workspace [`AGENTS.md`](../AGENTS.md); the filter-algebra exclusion
decision is bd JMAP-lbdy.

## Spec coverage

| Feature | Status |
|---|---|
| `AddressBook` object (RFC 9610 §2) | Complete |
| `AddressBookRights` (4 boolean fields) | Complete |
| `ContactCard` JMAP wrapper (RFC 9610 §3) | Complete (sub-objects as `Value`) |
| `ContactCardFilterCondition` with slash-keyed fields | Complete |
| `ContactCardComparator` | Complete |
| `AddressBookProperty` / `ContactCardProperty` enums | Complete |
| `ContactsCapability` / `ContactsAccountCapability` | Complete |
| Typed JSContact sub-object structs (RFC 9553 §2) | Available via re-export from `jmap-jscontact-types` |
| vCard/jCard import-export | Out of scope |
| JSContact-to-vCard conversion (RFC 9555) | Out of scope |

## How to use

### AddressBook deserialization

```rust
use jmap_contacts_types::AddressBook;

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
```

### ContactCard deserialization

`ContactCard` wraps the JSContact `Card` format. The top-level JMAP fields
(`id`, `addressBookIds`) are typed. Most JSContact sub-object fields (`name`,
`emails`, `phones`, `addresses`, etc.) are `serde_json::Value` — see
[Known Limitations](#known-limitations) below.

```rust
use jmap_contacts_types::ContactCard;

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
            "contexts": { "private": true },
            "address": "joe.bloggs@example.com"
        }
    }
}"#;

let card: ContactCard = serde_json::from_str(json).unwrap();

// JMAP fields are typed:
let id = card.id.as_ref().unwrap();
assert_eq!(id.as_ref(), "3");

// JSContact sub-objects are serde_json::Value — index into them directly:
let emails = card.emails.as_ref().unwrap();
assert_eq!(emails["0"]["address"], "joe.bloggs@example.com");
```

To extract a typed view of a sub-object, deserialize the `Value` into the
matching type from `jmap-jscontact-types` (re-exported at this crate's
root):

```rust
use jmap_contacts_types::{ContactCard, EmailAddress, Name};
use std::collections::HashMap;

let json = r#"{
    "id": "3",
    "name": {
        "components": [
            { "kind": "given", "value": "Joe" },
            { "kind": "surname", "value": "Bloggs" }
        ],
        "isOrdered": true
    },
    "emails": {
        "0": {
            "contexts": { "private": true },
            "address": "joe.bloggs@example.com"
        }
    }
}"#;
let card: ContactCard = serde_json::from_str(json).unwrap();

// Single-object sub-fields deserialize into a single type:
let name: Name = serde_json::from_value(card.name.unwrap()).unwrap();
assert_eq!(name.components.as_ref().unwrap()[0].value, "Joe");

// Map sub-fields deserialize into HashMap<Id, T>:
let emails: HashMap<String, EmailAddress> =
    serde_json::from_value(card.emails.unwrap()).unwrap();
assert_eq!(emails["0"].address, "joe.bloggs@example.com");
```

The full list of re-exported sub-types is in
[`jmap-jscontact-types`'s README](../crate-jmap-jscontact-types/README.md).

## JSContact sub-objects

JSContact sub-object types are available three ways from this crate:

- Top-level: `jmap_contacts_types::Name`, `jmap_contacts_types::EmailAddress`,
  etc. (recommended for application code).
- Module alias: `jmap_contacts_types::jscontact::Name` (mirrors
  `jmap_calendars_types::jscalendar::*`).
- Direct: `jmap_jscontact_types::Name` (lowest-coupling option for
  callers that already depend on `jmap-jscontact-types` separately).

All three paths resolve to the same type.

## How it works

### camelCase serde

All structs carry `#[serde(rename_all = "camelCase")]`. Wire field names match
the JMAP and JSContact specs exactly (`addressBookIds`, `myRights`, `mayRead`,
etc.).

### addressBookIds wire format

`addressBookIds` is an `Id[Boolean]` map — a JSON object where each key is an
`AddressBook` Id and each value is `true`. Rust type: `Option<HashMap<Id,
bool>>`. The standard serde `HashMap` serializer handles this correctly.

### JSContact sub-object design

The `ContactCard` struct has `Option<serde_json::Value>` for every JSContact
collection field (`name`, `emails`, `phones`, `addresses`, `organizations`,
`titles`, `notes`, `links`, `calendars`, `cryptoKeys`, `media`,
`preferredLanguages`, `anniversaries`, `keywords`, etc.) — keeping the wire
format as the anchor so that deserialization is infallible for partial JMAP
responses and unknown-shape extensions.

Typed Rust structs for JSContact sub-objects live in the sibling
[`jmap-jscontact-types`] crate and are re-exported here. Callers obtain
typed views by deserializing the relevant `serde_json::Value` into the
corresponding type — see the
[`ContactCard` deserialization](#contactcard-deserialization) example and
the [JSContact sub-objects](#jscontact-sub-objects) section above.

### Property enums

`AddressBookProperty` and `ContactCardProperty` enumerate the property names
recognized by `AddressBook/get` and `ContactCard/get` requests, respectively.
They are open-ended (a `Custom(String)` variant accepts vendor extensions) and
serialize to the JSContact wire-format property name (including slash-keyed
JSContact paths such as `name/given`).

## Gotchas

### JSContact sub-object fields on `ContactCard` are `serde_json::Value` on the wire

The following `ContactCard` fields are all `Option<serde_json::Value>` so
that round-trip fidelity is preserved across unknown-shape spec extensions:

`name`, `nicknames`, `emails`, `phones`, `addresses`, `organizations`,
`titles`, `notes`, `links`, `calendars`, `schedulingAddresses`,
`cryptoKeys`, `directories`, `media`, `preferredLanguages`,
`anniversaries`, `keywords`, `personalInfo`, `relatedTo`, `speakToAs`.

Typed access is **opt-in** via the re-exported types from
[`jmap-jscontact-types`] — see the
[JSContact sub-objects](#jscontact-sub-objects) section. The wire format
itself is unchanged.

### No compile-time enforcement of JSContact field constraints

Even when callers use the re-exported typed sub-objects, semantic
constraints from RFC 9553 (required-when-other-set, value ranges, format
strings) are not enforced at the type level. Constraint enforcement is
the responsibility of the method handler layer (`jmap-contacts-server`)
or the caller.

## Crate family

```
jmap-types (RFC 8620 wire primitives)
    └── jmap-contacts-types  ← this crate
            ├── jmap-contacts-server (method handlers)
            └── jmap-contacts-client (client extension trait)

jmap-jscontact-types (RFC 9553 JSContact typed sub-types, no JMAP dep)
    └── jmap-contacts-types  ← (this crate also depends here)
```

Path dependencies between crates use `path = "../crate-jmap-*"` and will
remain that way until the family is published to crates.io.

## References

- **[RFC 9610]** — JMAP Contacts (normative for
  AddressBook, ContactCard JMAP binding, filter, capability)
- **[RFC 9553]** — JSContact (normative for all Card sub-object types)
- **[RFC 8620]** — JMAP Core (request format, Id, UTCDate, State, Filter)

[RFC 9610]: https://www.rfc-editor.org/rfc/rfc9610
[RFC 9553]: https://www.rfc-editor.org/rfc/rfc9553
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
