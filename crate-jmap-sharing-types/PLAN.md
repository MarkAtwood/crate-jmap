# jmap-sharing-types — Implementation Plan

Cross-cutting sharing primitives for the JMAP ecosystem.

## Spec

- `~/PROJECT/jmap-chat-spec/references/rfc9670.txt` — JMAP Sharing (normative, published Nov 2024)
- `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-mail-sharing-00.txt` — applies RFC 9670 to RFC 8621 Mailbox

## Crate Family Position

```
jmap-types
    └── jmap-sharing-types  ← this crate
            ├── jmap-sharing-server
            └── jmap-sharing-client
```

## What This Crate Is

Serde-serializable data types for JMAP Sharing (RFC 9670):
- `Principal` — a user, group, resource, or location that can own or be
  shared with. Used as the target of `shareWith` maps in all shareable
  data types (Mailbox, Calendar, AddressBook, FileNode, etc.)
- `ShareNotification` — an inbox-style notification received when
  another user shares data with you

No async, no I/O. Depends only on `jmap-types` and `serde`.

## Key Types (RFC 9670)

### Principal (RFC 9670 §2)

```
id           Id          server-assigned
type         String      "individual" | "group" | "resource" | "location" | "other"
name         String      human-readable display name
description  String?     optional description
email        String?     email address (for individual principals)
timeZone     String?     IANA time zone identifier
capabilities Object?     per-capability settings for this principal
accounts     Id[Account]? JMAP accounts owned by this principal
```

### ShareNotification (RFC 9670 §3)

```
id           Id
created      UTCDate
changedBy    { name, email, principalId? }
objectType   String      e.g. "Mailbox", "Calendar", "AddressBook"
objectAccountId  Id
objectId     Id
oldRights    String[Boolean]?   rights before the change
newRights    String[Boolean]?   rights after the change (null = access removed)
```

### Sharing framework properties (RFC 9670 §4)

All shareable data types MUST define:
- `isSubscribed: Boolean` — has the user indicated they want to see this?
- `myRights: String[Boolean]` — the calling user's current permissions
- `shareWith: Id[String[Boolean]]` — map of principal id → rights

These properties are defined on the individual domain types (Mailbox,
Calendar, etc.), not in this crate. This crate defines only the
Principal and ShareNotification objects themselves.

## Usage in Other Crates

When `jmap-mail-types` adds mailbox sharing support (per
`draft-ietf-jmap-mail-sharing-00`), it will add:

```rust
pub share_with: Option<HashMap<Id, MailboxRights>>,
```

to `Mailbox`. The `PrincipalId` is just a `jmap_types::Id` — no direct
dependency on this crate required for the `shareWith` map key.

## Source Material

RFC 9670 is the published (Nov 2024) successor to
`draft-ietf-jmap-sharing-00`. Use RFC 9670 as the normative source.
The mail-sharing draft extends RFC 8621 §2 (Mailbox) to add the three
sharing properties; read it alongside RFC 9670.
