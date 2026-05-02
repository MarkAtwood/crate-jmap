# jmap-contacts-types — Implementation Plan

Data types for the JMAP Contacts extension.

## Spec

- `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-contacts-10.txt`
- `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-jscontact-10.txt`
- `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-jscontact-vcard-09.txt`

## Crate Family Position

```
jmap-types
    └── jmap-contacts-types  ← this crate
            ├── jmap-contacts-server
            └── jmap-contacts-client
```

## What This Crate Is

Serde-serializable data types for JMAP Contacts:
- `Contact` — JSContact Card object (RFC 9553)
- `ContactGroup` — group of contacts
- `AddressBook` — container for contacts (analogous to Mailbox for email)

No async, no I/O. Depends only on `jmap-types` and `serde`.

## Key Types (draft-ietf-jmap-contacts-10)

- `AddressBook` — `id`, `name`, `isDefault`, `isSubscribed`, `shareWith`, `myRights`
- `Contact` / `Card` — JSContact Card (name, emails, phones, addresses, etc.)
- `ContactGroup` — `id`, `name`, `cardIds`

## Source Material

The JSContact Card format is defined in RFC 9553 (JSContact). The JMAP binding is in
draft-ietf-jmap-contacts-10. The jscontact-vcard draft defines round-trip conversion.
