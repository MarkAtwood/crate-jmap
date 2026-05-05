# jmap-contacts-types

Serde-annotated Rust types for JMAP Contacts ([RFC 9610]) and
JSContact ([RFC 9553]). Types only — no method handlers, no async, no network I/O.

## What

| Type | Module | Source |
|---|---|---|
| `AddressBook` | `addressbook` | contacts-10 §2 |
| `AddressBookRights` | `addressbook` | contacts-10 §2 |
| `ContactCard` | `card` | contacts-10 §3, RFC 9553 §2 |
| `ContactCardFilterCondition` | `card` | contacts-10 §3.3.1 |
| `ContactCardComparator` | `card` | contacts-10 §3.3 |
| `ContactsCapability` | `capability` | contacts-10 §1.4.1 |
| `ContactsAccountCapability` | `capability` | contacts-10 §1.4.1 |
| `Name`, `NameComponent` | `jscontact::name` | RFC 9553 §2.2.1 |
| `Nickname` | `jscontact::nickname` | RFC 9553 §2.2.2 |
| `Organization`, `OrgUnit` | `jscontact::org` | RFC 9553 §2.2.3 |
| `SpeakToAs`, `Pronouns` | `jscontact::speak_to` | RFC 9553 §2.2.4 |
| `Title` | `jscontact::title` | RFC 9553 §2.2.5 |
| `EmailAddress` | `jscontact::email` | RFC 9553 §2.3.1 |
| `OnlineService` | `jscontact::online` | RFC 9553 §2.3.2 |
| `Phone` | `jscontact::phone` | RFC 9553 §2.3.3 |
| `LanguagePref` | `jscontact::lang` | RFC 9553 §2.3.4 |
| `Address`, `AddressComponent` | `jscontact::address` | RFC 9553 §2.5.1 |
| `Calendar`, `SchedulingAddress` | `jscontact::calendar` | RFC 9553 §2.4 |
| `CryptoKey`, `Directory`, `Link`, `Media` | `jscontact::resource` | RFC 9553 §2.6 |
| `Anniversary`, `PartialDate` | `jscontact::anniversary` | RFC 9553 §2.8.1 |
| `Note` | `jscontact::note` | RFC 9553 §2.8.3 |
| `PersonalInfo` | `jscontact::personal` | RFC 9553 §2.8.4 |
| `Relation` | `jscontact::relation` | RFC 9553 §2.1.8 |

## Spec coverage

| Feature | Status |
|---|---|
| `AddressBook` object (contacts-10 §2) | Complete |
| `AddressBookRights` (4 boolean fields) | Complete |
| `ContactCard` JMAP wrapper (contacts-10 §3) | Complete |
| JSContact `Name`, `Organization`, `Title` | Complete |
| JSContact `EmailAddress`, `Phone`, `OnlineService` | Complete |
| JSContact `Address` | Complete |
| JSContact `Calendar`, `SchedulingAddress` | Complete |
| JSContact `CryptoKey`, `Directory`, `Link`, `Media` | Complete |
| JSContact `Anniversary`, `PartialDate` | Complete |
| JSContact `Note`, `PersonalInfo`, `Relation` | Complete |
| `ContactCardFilterCondition` with slash-keyed fields | Complete |
| `ContactsCapability` / `ContactsAccountCapability` | Complete |
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

Typed sub-object structs (`Name`, `EmailAddress`, `Phone`, `Address`, etc.) are
defined in the `jscontact/` module and are exported from the crate root. They
can be used independently — for example, when the caller knows a field is
present and wants to deserialize it further:

```rust
use jmap_contacts_types::jscontact::EmailAddress;
// (hypothetical — the field is Value on ContactCard)
```

### Enum open-endedness

JSContact allows vendor-specific extension values in many string-enum fields.
These are represented as `String` with named constants in a `mod consts`
submodule, rather than closed Rust enums. Only sets that the spec declares
non-extensible use actual enums; none exist in this spec.

### @type discriminators

`@type` fields on JSContact sub-objects are serialized with the correct literal
value but not validated on deserialization. The type is implied by the context
(an object in the `emails` map is always an `EmailAddress`).

## Known Limitations

### JSContact sub-objects are `serde_json::Value` on ContactCard

The following `ContactCard` fields are all `Option<serde_json::Value>`:

`names`, `nicknames`, `emails`, `phones`, `addresses`, `organizations`,
`titles`, `notes`, `links`, `calendars`, `cryptoKeys`, `photos`,
`preferredLanguages`, `localizations`, `anniversaries`, `keywords`,
`extensions`

Callers needing typed access to contact fields (e.g., to extract a phone
number or postal address) must deserialize the `Value` themselves using
RFC 9553 as the schema. The typed structs (`Phone`, `Address`, etc.) in the
`jscontact/` module exist and can be used for this purpose.

### No compile-time enforcement of JSContact field constraints

Because sub-object fields are `serde_json::Value`, invalid sub-object JSON
silently round-trips. Constraint enforcement (required fields, value ranges,
format strings) is the responsibility of the method handler layer
(`jmap-contacts-server`) or the caller.

### Typed structs for ContactCard sub-objects are a future goal

Typed Rust structs for JSContact sub-objects matching RFC 9553 §2.x are
defined in the `jscontact/` module but are not yet used as the field types on
`ContactCard` itself. The gap between the defined types and the `Value` fields
on `ContactCard` is intentional and tracked for a future release.

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
