# jmap-contacts-types

Serde-annotated Rust types for JMAP Contacts ([RFC 9610]) and
JSContact ([RFC 9553]). Types only — no method handlers, no async, no network I/O.

## What

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

JSContact sub-objects (`Name`, `EmailAddress`, `Phone`, `Address`, etc.) are
**not** exported as typed structs from this crate. Sub-object fields on
`ContactCard` are `Option<serde_json::Value>` — see
[Known Limitations](#known-limitations) below.

## Filter extensibility

Filter and comparator types in this crate — `ContactCardFilterCondition`,
`ContactCardComparator`, and the generic `Filter<T>` / `Operator` re-exported
from `jmap-types` — are **intentionally not extensible** via vendor "extras"
fields. A filter clause the server does not understand silently breaks query
correctness: the client gets the wrong set of records back with no error
signal. So these types deliberately have no `extra` catch-all field.

Vendors who need to filter on custom fields have two options:

- **IETF-track (recommended).** Use `draft-ietf-jmap-metadata` (capability URI
  `urn:ietf:params:jmap:metadata`), which defines a `Metadata` / `Annotation`
  companion object keyed by `(relatedType, relatedId)` with capability-declared
  schema (`metadataTypes` / `maxDepth`) and a `Metadata/query` `textMatch`
  filter. This is the workspace's recommended path for vendor data that needs
  to be queryable; the implementation tracker is bd JMAP-06zp.
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
| Typed JSContact sub-object structs (RFC 9553 §2) | Not provided — fields are `serde_json::Value` |
| vCard/jCard import-export | Out of scope |
| JSContact-to-vCard conversion (RFC 9555) | Out of scope |

## Usage

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

To extract a specific field from a sub-object, deserialize the `Value` against
your own struct or use RFC 9553 as the schema for manual `Value` access.

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
`titles`, `notes`, `links`, `calendars`, `cryptoKeys`, `photos`,
`preferredLanguages`, `localizations`, `anniversaries`, `keywords`,
`extensions`). This keeps deserialization infallible for partial JMAP responses
and for JSContact extensions the crate does not know about.

Typed Rust structs for JSContact sub-objects are **not** provided by this
crate. Callers needing typed access to a sub-object (e.g. to extract a phone
number or postal address) must deserialize the relevant `serde_json::Value`
into their own struct, using RFC 9553 as the schema.

### Property enums

`AddressBookProperty` and `ContactCardProperty` enumerate the property names
recognized by `AddressBook/get` and `ContactCard/get` requests, respectively.
They are open-ended (a `Custom(String)` variant accepts vendor extensions) and
serialize to the JSContact wire-format property name (including slash-keyed
JSContact paths such as `name/given`).

## Known Limitations

### JSContact sub-objects are `serde_json::Value` on ContactCard

The following `ContactCard` fields are all `Option<serde_json::Value>`:

`name`, `nicknames`, `emails`, `phones`, `addresses`, `organizations`,
`titles`, `notes`, `links`, `calendars`, `cryptoKeys`, `photos`,
`preferredLanguages`, `localizations`, `anniversaries`, `keywords`,
`extensions`

Callers needing typed access to contact fields (e.g., to extract a phone
number or postal address) must deserialize the `Value` themselves into a
struct of their own, using RFC 9553 as the schema. No typed sub-object
structs are exported from this crate.

### No compile-time enforcement of JSContact field constraints

Because sub-object fields are `serde_json::Value`, invalid sub-object JSON
silently round-trips. Constraint enforcement (required fields, value ranges,
format strings) is the responsibility of the method handler layer
(`jmap-contacts-server`) or the caller.

### Typed structs for ContactCard sub-objects are a future goal

Typed Rust structs for JSContact sub-objects matching RFC 9553 §2.x are
intentionally not provided in this release. The gap is tracked for a future
release; until then, callers work with the `serde_json::Value` fields on
`ContactCard` directly.

## Crate family

```
jmap-types (RFC 8620 wire primitives)
    └── jmap-contacts-types  ← this crate
            ├── jmap-contacts-server (method handlers)
            └── jmap-contacts-client (client extension trait)
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

## License

MIT OR Apache-2.0
