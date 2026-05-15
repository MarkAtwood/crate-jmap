# jmap-jscontact-types

RFC 9553 JSContact typed sub-types for the `jmap-*` crate family.

Consumed by `jmap-contacts-types`. Pure data types: no method handlers,
no async, no network I/O.

## What it is

This crate provides the JSContact sub-object types defined in RFC 9553.
They are embedded inside the `ContactCard` JMAP object (`jmap-contacts-types`)
and have no JMAP identity of their own.

| Type(s) | RFC 9553 § |
|---|---|
| `Name`, `NameComponent` | §2.2.1 |
| `Nickname` | §2.2.2 |
| `Organization`, `OrgUnit` | §2.2.3 |
| `SpeakToAs`, `Pronouns` | §2.2.4 |
| `Title` | §2.2.5 |
| `EmailAddress` | §2.3.1 |
| `OnlineService` | §2.3.2 |
| `Phone` | §2.3.3 |
| `LanguagePref` | §2.3.4 |
| `Calendar` | §2.4.1 |
| `SchedulingAddress` | §2.4.2 |
| `Address`, `AddressComponent` | §2.5.1 |
| `CryptoKey` | §2.6.1 |
| `Directory` | §2.6.2 |
| `Link` | §2.6.3 |
| `Media` | §2.6.4 |
| `Anniversary`, `PartialDate`, `Timestamp` | §2.8.1 |
| `Note`, `Author` | §2.8.3 |
| `PersonalInfo` | §2.8.4 |
| `Relation` | §2.1.8 |

## Why a separate crate

Following the precedent of `jmap-jscalendar-types`: JSContact (RFC 9553)
is a standalone IETF spec consumed by JMAP Contacts and potentially other
future consumers. The Rust crate graph mirrors the spec dep graph.

`jmap-contacts-types` re-exports these typed sub-types so callers can do
`jmap_contacts_types::Name` symmetrically with the jscalendar pattern.

## What it's for

These typed sub-types are consumed by `jmap-contacts-types`, which re-exports
them as the `jscontact` module alias so callers can write
`jmap_contacts_types::Name` symmetrically with the jscalendar pattern. The
crate carries pure data-types only: no JMAP-method semantics, no async, no
network I/O. It has no JMAP dependency — the RFC 9553 sub-object types are
defined independently of the JMAP wire envelope.

## How to use

```toml
[dependencies]
jmap-jscontact-types = "0.1"
```

Transitively pulls in `serde`, `serde_json`. No JMAP dependency. Parse a
`Name` directly from a JSON `Value`:

```rust
use jmap_jscontact_types::{Name, NameComponent};
use serde_json::json;

let name: Name = serde_json::from_value(json!({
    "@type": "Name",
    "components": [
        {"@type": "NameComponent", "kind": "given", "value": "Ada"},
        {"@type": "NameComponent", "kind": "surname", "value": "Lovelace"}
    ],
    "full": "Ada Lovelace"
}))?;
# Ok::<(), serde_json::Error>(())
```

The same pattern works for `EmailAddress`, `Phone`, `Address`,
`Organization`, `Anniversary`, and the other sub-types in this crate.

## How it works

- Sealed sub-type set: every public sub-type that appears on the wire is
  defined in this crate's `src/lib.rs` and matches the RFC 9553 §2.2 / §2.3 /
  §2.4 / §2.5 / §2.6 / §2.8 boundary.
- `#[non_exhaustive]` on every public struct, so additive spec evolution is
  a non-breaking change.
- Wire field `"@type"` is mapped to the Rust field
  `at_type: Option<String>` via `#[serde(rename = "@type", default,
  skip_serializing_if = "Option::is_none")]`. The field is kept as
  `String` (not a closed enum) for forward-compatibility with new
  sub-object types, and wrapped in `Option` because RFC 9553 §1.3.4
  permits omitting `@type` in `defaultType` positions (notably
  `Anniversary.date` defaulting to `PartialDate`).
- `#[serde(rename_all = "camelCase")]` on all structs.
- No async. `#[forbid(unsafe_code)]` at the crate root.
- Dependencies limited to `serde`, `serde_json` — no JMAP dep.

## Gotchas

- Classifier-attribute strings (`NameComponent.kind`, `Title.kind`,
  `Calendar.kind`, `Anniversary.kind`, `PersonalInfo.kind`) are typed as
  `Option<String>` / `String`, not as `enum { … , Other(String) }`.
  This is per the workspace `AGENTS.md`
  "externally-owned-schema classifier strings" exclusion: real-world
  exporters (Outlook, Google, Apple, vCard converters, localized
  clients) routinely send values outside the RFC 9553 enumeration, so
  the catch-all would do all the real work for no programmatic
  dispatch benefit. Match string literals if you need to dispatch.
- `JsContactId` is a transparent newtype around `String` and does NOT
  validate the RFC 9553 §1.4.1 character set (`A-Z`, `a-z`, `0-9`,
  `-`, `_`, length 1–255). Validation is left to the caller because
  this crate has no JMAP dependency.
- The Sloppy-Value pattern in `jmap-contacts-types` (per the workspace
  `AGENTS.md`) means consumers see some contact-card fields as
  `serde_json::Value` rather than typed sub-types from this crate. To
  reach the typed shape, deserialise the value through one of the
  types here (e.g. `serde_json::from_value::<Address>(...)`).
- There is no public `Resource` trait or generic across the five
  Resource-derived types (`Calendar`, `CryptoKey`, `Directory`,
  `Link`, `Media`). RFC 9553 §1.4.4 defines `Resource` as a
  documentation-only abstract type; the wire format is a flat object
  per concrete type and the crate mirrors that. Consumers that want
  to write a single helper accepting any Resource-derived type need
  to define their own outer enum or trait. See `PLAN.md`
  §"Resource-derived types" for the rationale.

## References

- [RFC 9553] — JSContact (normative for every type in this crate)
  - §1.4 — common data types and identifiers
  - §2.2 — Name, Nickname, Organization, SpeakToAs, Title
  - §2.3 — EmailAddress, OnlineService, Phone, LanguagePref
  - §2.4 — Calendar, SchedulingAddress
  - §2.5 — Address, AddressComponent
  - §2.6 — CryptoKey, Directory, Link, Media
  - §2.8 — Anniversary, Note, PersonalInfo

[RFC 9553]: https://www.rfc-editor.org/rfc/rfc9553
