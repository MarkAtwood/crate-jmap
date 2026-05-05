# jmap-jscontact

Typed Rust structs for [RFC 9553] (JSContact) sub-objects, for use with
`jmap-contacts-types`.

## Design

`ContactCard` in `jmap-contacts-types` stores all JSContact sub-objects
(`name`, `phones`, `emails`, `addresses`, etc.) as `serde_json::Value` for
round-trip fidelity — vendor extension fields and future RFC revisions are
preserved without data loss.

This crate provides typed structs for all RFC 9553 §2 sub-objects. Callers use
them via explicit conversion using the `parse_*` helper functions:

```rust
use jmap_jscontact::{parse_name, parse_emails, Name, EmailAddress};

// card is a jmap_contacts_types::ContactCard

// Get a typed Name:
let name: Option<Name> = parse_name(card.name.as_ref());

if let Some(name) = name {
    for component in name.components.iter().flatten() {
        println!("{:?}: {}", component.kind, component.value.as_deref().unwrap_or(""));
    }
    if let Some(full) = &name.full {
        println!("Full name: {full}");
    }
}

// Get typed email addresses:
let emails = parse_emails(card.emails.as_ref());
for (id, email) in &emails {
    println!("{id}: {}", email.address.as_deref().unwrap_or(""));
}
```

Unknown vendor-specific fields are preserved in the `extra: HashMap<String,
Value>` field on every struct, so round-trips through `serde_json::from_value`
/ `serde_json::to_value` do not lose data.

## All `parse_*` helpers

| Function | Input field | Return type |
|---|---|---|
| `parse_name` | `card.name` | `Option<Name>` |
| `parse_nicknames` | `card.nicknames` | `HashMap<String, Nickname>` |
| `parse_organizations` | `card.organizations` | `HashMap<String, Organization>` |
| `parse_speak_to_as` | `card.speak_to_as` | `Option<SpeakToAs>` |
| `parse_titles` | `card.titles` | `HashMap<String, Title>` |
| `parse_emails` | `card.emails` | `HashMap<String, EmailAddress>` |
| `parse_online_services` | `card.online_services` | `HashMap<String, OnlineService>` |
| `parse_phones` | `card.phones` | `HashMap<String, Phone>` |
| `parse_preferred_languages` | `card.preferred_languages` | `HashMap<String, LanguagePref>` |
| `parse_calendars` | `card.calendars` | `HashMap<String, Calendar>` |
| `parse_scheduling_addresses` | `card.scheduling_addresses` | `HashMap<String, SchedulingAddress>` |
| `parse_addresses` | `card.addresses` | `HashMap<String, Address>` |
| `parse_crypto_keys` | `card.crypto_keys` | `HashMap<String, CryptoKey>` |
| `parse_directories` | `card.directories` | `HashMap<String, Directory>` |
| `parse_links` | `card.links` | `HashMap<String, Link>` |
| `parse_media` | `card.media` | `HashMap<String, Media>` |
| `parse_anniversaries` | `card.anniversaries` | `HashMap<String, Anniversary>` |
| `parse_notes` | `card.notes` | `HashMap<String, Note>` |
| `parse_personal_info` | `card.personal_info` | `HashMap<String, PersonalInfo>` |
| `parse_related_to` | `card.related_to` | `HashMap<String, Relation>` |

All helpers return empty maps / `None` if the field is absent, `null`, or
cannot be deserialized (rather than panicking or returning `Err`).

## Types

All sub-object types are in these modules:

| Module | Types |
|---|---|
| `name` | `Name`, `NameComponent`, `NameComponentKind`, `PhoneticSystem`, `Nickname`, `Organization`, `OrgUnit`, `SpeakToAs`, `GrammaticalGender`, `Pronouns`, `Title`, `TitleKind` |
| `contact` | `EmailAddress`, `OnlineService`, `Phone`, `LanguagePref`, `Calendar`, `CalendarKind`, `SchedulingAddress` |
| `address` | `Address`, `AddressComponent`, `AddressComponentKind` |
| `resource` | `CryptoKey`, `Directory`, `DirectoryKind`, `Link`, `LinkKind`, `Media`, `MediaKind` |
| `anniversary` | `Anniversary`, `AnniversaryKind`, `AnniversaryDate`, `PartialDate`, `Timestamp` |
| `note` | `Note`, `Author` |
| `personal` | `PersonalInfo`, `PersonalInfoKind`, `PersonalInfoLevel`, `Relation` |

All types are re-exported from the crate root.

## Known Limitations

- All fields are `Option<T>` even when the RFC marks them mandatory, because
  JMAP `properties` filtering can omit any field from a partial response.
  Constraint enforcement (required fields, value ranges) is the caller's
  responsibility.
- `AnniversaryDate` uses `#[serde(untagged)]` discrimination (Timestamp is
  tried first due to its unique `utc` field). If a future spec adds a third
  variant with overlapping fields, disambiguation may break.
- `CryptoKey.kind` is `Option<String>` (not an enum) because RFC 9553 §2.6.1
  defines no fixed vocabulary for this field.
- Phone `features` and `contexts` on multiple types use `HashMap<String, bool>`
  (not enums) because these maps are extensible per the spec.

## References

- [RFC 9553] — JSContact (normative)
- [RFC 8620] — JMAP Core (Id type)
- [draft-ietf-jmap-contacts-10] — JMAP Contacts (ContactCard uses these types)

[RFC 9553]: https://www.rfc-editor.org/rfc/rfc9553
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[draft-ietf-jmap-contacts-10]: https://www.ietf.org/archive/id/draft-ietf-jmap-contacts-10.txt

## License

MIT OR Apache-2.0
