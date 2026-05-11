# jmap-jscontact-types

RFC 9553 JSContact typed sub-types for the `jmap-*` crate family.

Consumed by `jmap-contacts-types`. Pure data types: no method handlers,
no async, no network I/O.

## What

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

## Dependencies

```toml
jmap-jscontact-types = "0.1"
```

Transitively pulls in `serde`, `serde_json`. No JMAP dependency.

## License

MIT OR Apache-2.0
