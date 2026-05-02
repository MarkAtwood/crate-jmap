# jmap-mail-types

RFC 8621 (JMAP for Mail) data types for the `jmap-*` crate family.

Implements every object type defined by [RFC 8621]: `Mailbox`, `Thread`, `Email`,
`Identity`, `EmailSubmission`, `SearchSnippet`, and `VacationResponse`. All types
serialize to and from the exact JSON wire format the spec requires.

This crate is **types only** — no method handlers, no async, no network I/O. It sits
between `jmap-types` (shared wire primitives from RFC 8620) and `jmap-mail-server`
(the method handler layer).

## What

| Type | RFC 8621 § | Description |
|------|-----------|-------------|
| `Mailbox` | §2 | Folder/label container: `id`, `name`, `role`, `sortOrder`, `totalEmails`, `unreadEmails`, `totalThreads`, `unreadThreads`, `myRights`, `isSubscribed` |
| `MailboxRights` | §2.1 | Per-mailbox ACL: `mayReadItems`, `mayAddItems`, `mayRemoveItems`, `maySetSeen`, `maySetKeywords`, `mayCreateChild`, `mayRename`, `mayDelete`, `maySubmit` |
| `MailboxRole` | §2.1 | Typed enum: `Inbox`, `Drafts`, `Sent`, `Trash`, `Junk`, `Archive`, `Flagged`, `Important`, `All`, `Other(String)` |
| `MailboxFilterCondition` | §2.3 | Filter condition for `Mailbox/query` |
| `Thread` | §3 | `id` + `emailIds: Vec<Id>`, sorted oldest-first |
| `Email` | §4 | A full email message; metadata fields (`blobId`, `threadId`, `mailboxIds`, `keywords`, `size`, `receivedAt`) plus optional parsed-header and body fields |
| `EmailAddress` | §4.1.2 | `{ name, email }` address pair from an RFC 5322 address-list |
| `EmailAddressGroup` | §4.1.2 | Named group of `EmailAddress` values, preserving RFC 5322 group structure |
| `EmailHeader` | §4.1.3 | Raw header field: `{ name, value }` |
| `EmailBodyPart` | §4.1.4 | One MIME body part: `partId`, `blobId`, `type`, `charset`, `size`, `headers`, `subParts` |
| `EmailBodyValue` | §4.1.4 | Decoded text content of a body part: `value`, `isEncodingProblem`, `isTruncated` |
| `Keyword` | §4.1.1 | Newtype over `String`; system constants in the `keyword` module (`$seen`, `$flagged`, etc.) |
| `EmailFilter` / `Filter<T>` | §4.4 | `FilterCondition` or `FilterOperator` (`and`/`or`/`not`) union type |
| `EmailFilterCondition` | §4.4.1 | Per-email filter: mailbox, keyword, date range, size, text search, header match |
| `EmailComparator` | §4.4.2 | Sort comparator: `property`, `isAscending`, `collation`, `keyword` |
| `Identity` | §6 | Send-from identity: `id`, `name`, `email`, `replyTo`, `bcc`, `textSignature`, `htmlSignature`, `mayDelete` |
| `EmailSubmission` | §7 | Outbound send record: `id`, `identityId`, `emailId`, `threadId`, `envelope`, `sendAt`, `undoStatus`, `deliveryStatus` |
| `Envelope` | §7 | SMTP envelope: `mailFrom` and `rcptTo` |
| `Address` | §7 | SMTP address with optional `parameters` map |
| `DeliveryStatus` | §7.1 | Per-recipient status: `smtpReply`, `delivered`, `displayed` |
| `Delivered` | §7.1 | Enum: `Queued`, `Yes`, `No`, `Unknown`, `Other(String)` |
| `Displayed` | §7.1 | Enum: `Unknown`, `Yes`, `Other(String)` |
| `UndoStatus` | §7 | Enum: `Pending`, `Final`, `Canceled`, `Other(String)` |
| `SearchSnippet` | §5 | `emailId` + optional `subject` / `preview` strings with `<mark>` highlights |
| `VacationResponse` | §8 | Singleton auto-reply: `isEnabled`, `fromDate`, `toDate`, `subject`, `textBody`, `htmlBody` |
| `EmailProperty` | — | Enum of legal `Email` property names for `properties` arrays |
| `MailboxProperty` | — | Enum of legal `Mailbox` property names |
| `ThreadProperty` | — | Enum of legal `Thread` property names |
| `IdentityProperty` | — | Enum of legal `Identity` property names |
| `EmailSubmissionProperty` | — | Enum of legal `EmailSubmission` property names |
| `SearchSnippetProperty` | — | Enum of legal `SearchSnippet` property names |
| `VacationResponseProperty` | — | Enum of legal `VacationResponse` property names |

## Why

`jmap-mail-server` needs these types to implement method handlers. `jmap-mail-client`
needs them to build requests and parse responses. Neither should carry the other's
dependencies. `jmap-mail-types` has exactly four runtime dependencies — `jmap-types`,
`serde`, `serde_json`, and nothing else — and no async runtime, no HTTP framework,
and no application logic. Any crate in the `jmap-*` family can depend on it without cost.

## Usage

Add to `Cargo.toml`:

```toml
[dependencies]
jmap-mail-types = "0.1"
```

### Deserializing a Mailbox from JSON

The `Mailbox` struct uses camelCase field names on the wire. Deserialize it directly
with `serde_json`:

```rust
use jmap_mail_types::Mailbox;

let json = r#"{
    "id": "mb1",
    "name": "Inbox",
    "role": "inbox",
    "sortOrder": 10,
    "totalEmails": 42,
    "unreadEmails": 3,
    "totalThreads": 20,
    "unreadThreads": 2,
    "myRights": {
        "mayReadItems": true,
        "mayAddItems": true,
        "mayRemoveItems": true,
        "maySetSeen": true,
        "maySetKeywords": true,
        "mayCreateChild": true,
        "mayRename": true,
        "mayDelete": false,
        "maySubmit": false
    },
    "isSubscribed": true
}"#;

let mailbox: Mailbox = serde_json::from_str(json).unwrap();
assert_eq!(mailbox.name, "Inbox");
assert_eq!(mailbox.unread_emails, 3);
assert!(mailbox.my_rights.may_read_items);
assert!(!mailbox.my_rights.may_delete);
```

### Building an Email object

Use `Email::new` for the six required metadata fields, then set optional header and
body fields directly:

```rust
use std::collections::HashMap;
use jmap_types::{Id, UTCDate};
use jmap_mail_types::{Email, EmailAddress, keyword, Keyword};

let mut mailbox_ids = HashMap::new();
mailbox_ids.insert(Id::from("inbox-id"), true);

let mut email = Email::new(
    Id::from("em1"),
    Id::from("blob1"),
    Id::from("thread1"),
    mailbox_ids,
    12345,                                        // size in bytes
    UTCDate::from("2024-06-01T09:00:00Z"),
);

email.subject = Some("Hello from JMAP".into());
email.from = Some(vec![EmailAddress { name: Some("Alice".into()), email: "alice@example.com".into() }]);
email.to   = Some(vec![EmailAddress::new("bob@example.com")]);
email.keywords.insert(Keyword::from(keyword::SEEN), true);

let json = serde_json::to_string(&email).unwrap();
// {"id":"em1","blobId":"blob1","threadId":"thread1","mailboxIds":{"inbox-id":true},
//  "keywords":{"$seen":true},"size":12345,"receivedAt":"2024-06-01T09:00:00Z",
//  "subject":"Hello from JMAP","from":[{"name":"Alice","email":"alice@example.com"}],
//  "to":[{"email":"bob@example.com"}]}
```

### Keywords

`Keyword` is a newtype over `String` that serializes transparently. Well-known system
keywords are available as `&str` constants in the `keyword` module. Because `Keyword`
implements `Borrow<str>`, a `HashMap<Keyword, bool>` can be queried with a bare `&str`:

```rust
use jmap_mail_types::{keyword, Keyword};
use std::collections::HashMap;

let mut kw: HashMap<Keyword, bool> = HashMap::new();
kw.insert(Keyword::from(keyword::SEEN), true);
kw.insert(Keyword::from(keyword::FLAGGED), true);

assert!(kw.contains_key(keyword::SEEN));
assert!(!kw.contains_key(keyword::DRAFT));

// Custom keywords are also valid; they must not start with '$'.
kw.insert(Keyword::new("project-alpha"), true);
```

Constants available in the `keyword` module: `DRAFT`, `SEEN`, `FLAGGED`, `ANSWERED`,
`FORWARDED`, `PHISHING`, `JUNK`, `NOT_JUNK`.

### Filtering with `EmailFilter`

`EmailFilter` is a type alias for `Filter<EmailFilterCondition>`. A `Filter<T>` is
either a leaf `FilterCondition` or a `FilterOperator` that combines sub-filters with
`and`, `or`, or `not`. Because `EmailFilterCondition` is `#[non_exhaustive]`, build it
with `Default::default()` and then mutate the fields you need:

```rust
use jmap_mail_types::query::{EmailFilter, EmailFilterCondition, FilterOperator, Operator};
use jmap_mail_types::{keyword, Keyword};
use jmap_types::Id;

// Simple condition: unread emails in a specific mailbox
let mut cond = EmailFilterCondition::default();
cond.in_mailbox  = Some(Id::from("inbox-id"));
cond.not_keyword = Some(Keyword::from(keyword::SEEN));
let filter = EmailFilter::Condition(cond);

// Compound: inbox AND (flagged OR has attachment)
let filter = EmailFilter::Operator(FilterOperator::new(
    Operator::And,
    vec![
        EmailFilter::Condition({
            let mut c = EmailFilterCondition::default();
            c.in_mailbox = Some(Id::from("inbox-id"));
            c
        }),
        EmailFilter::Operator(FilterOperator::new(
            Operator::Or,
            vec![
                EmailFilter::Condition({
                    let mut c = EmailFilterCondition::default();
                    c.has_keyword = Some(Keyword::from(keyword::FLAGGED));
                    c
                }),
                EmailFilter::Condition({
                    let mut c = EmailFilterCondition::default();
                    c.has_attachment = Some(true);
                    c
                }),
            ],
        )),
    ],
));

let json = serde_json::to_string(&filter).unwrap();
// {"operator":"AND","conditions":[
//   {"inMailbox":"inbox-id"},
//   {"operator":"OR","conditions":[{"hasKeyword":"$flagged"},{"hasAttachment":true}]}
// ]}
```

### MailboxRole

`MailboxRole` is a forward-compatible string enum. Known roles map to named variants;
any other string becomes `Other(String)` and round-trips correctly:

```rust
use jmap_mail_types::MailboxRole;

let role: MailboxRole = serde_json::from_str(r#""inbox""#).unwrap();
assert_eq!(role, MailboxRole::Inbox);
assert_eq!(role.to_wire_str(), "inbox");

// Unknown role is preserved
let future: MailboxRole = serde_json::from_str(r#""scheduled""#).unwrap();
assert!(matches!(future, MailboxRole::Other(ref s) if s == "scheduled"));
```

### EmailSubmission

Track an outbound send attempt with `EmailSubmission`. The `UndoStatus` enum indicates
whether cancellation is still possible:

```rust
use jmap_types::{Id, UTCDate};
use jmap_mail_types::submission::{EmailSubmission, UndoStatus};

let sub = EmailSubmission::new(
    Id::from("sub1"),
    Id::from("identity1"),
    Id::from("email1"),
    Id::from("thread1"),
    UTCDate::from("2024-06-01T09:05:00Z"),
    UndoStatus::Pending,
);

assert!(matches!(sub.undo_status, UndoStatus::Pending));
assert!(sub.delivery_status.is_none()); // not yet tracked
```

## How it works

### Wire format

All structs carry `#[serde(rename_all = "camelCase")]`, mapping Rust field names
(`received_at`, `blob_id`, `my_rights`) to the camelCase JSON names the RFC mandates
(`receivedAt`, `blobId`, `myRights`). Fields that are absent or empty are skipped with
`#[serde(skip_serializing_if = "Option::is_none")]` or
`#[serde(skip_serializing_if = "…::is_empty")]`, matching RFC 8620 §5.1 semantics.

### `Keyword`

`Keyword` is a newtype over `String` with `#[serde(transparent)]`. It serializes
directly as a JSON string, so `HashMap<Keyword, bool>` round-trips as a JSON object
with string keys — exactly the `keywords` wire format in RFC 8621 §4.1.1. System
keywords are `&str` constants (e.g. `keyword::SEEN = "$seen"`). Because `Keyword`
implements `Borrow<str>` and `Deref<Target = str>`, the constants work directly as
`HashMap` lookup keys without constructing a `Keyword`.

### `MailboxRole` and the `impl_string_enum!` macro

`MailboxRole` (and `Delivered`, `Displayed`, `UndoStatus`, `ComparatorProperty`) are
backed by an internal `impl_string_enum!` macro. The macro generates `Serialize`,
`Deserialize`, and `Display` implementations from a wire-string-to-variant table. Any
string not in the table deserializes to `Other(String)`, which serializes back to the
same string — making these enums forward-compatible with server extensions and future
RFC revisions without any change to client code. The `#[non_exhaustive]` attribute on
the enum ensures that adding a new named variant in a future version of this crate does
not silently break `match` expressions in downstream crates.

### `Filter<T>` and `#[serde(untagged)]`

`Filter<T>` (re-exported from `jmap-types`) is defined as:

```
enum Filter<T> {
    Condition(T),
    Operator(FilterOperator<T>),
}
```

with `#[serde(untagged)]`. Serde tries `FilterOperator` first (distinguished by the
presence of the `"operator"` key) and falls through to `T` for everything else. RFC
8620 §4.4 guarantees a filter is either an operator object or a condition object, so
this unambiguously covers all valid inputs. Unknown fields in a condition object are
silently ignored (no `deny_unknown_fields`) — this is intentional and required for
correct `untagged` deserialization.

### `EmailBodyPart.type_`

The MIME content-type field is named `type` in the JSON wire format, but `type` is a
Rust keyword. The struct field is named `type_` (conventional Rust escape) and carries
`#[serde(rename = "type")]` so the wire name is correct.

### `*Property` enums

Each JMAP object type has a corresponding `*Property` enum (e.g. `EmailProperty`,
`MailboxProperty`) that enumerates the legal property names for `properties` arrays in
`/get` requests. These are consumed by `jmap-mail-server` when projecting partial
responses; they have no serde derive and are not present in the wire format.

### `#[non_exhaustive]` and constructors

All public structs are `#[non_exhaustive]`. This prevents external callers from using
struct literal syntax (which would otherwise lock in every field as a semver guarantee)
and allows new optional fields to be added without a breaking change. Each struct
provides a `new(…)` constructor that takes only the required fields; optional fields
default to `None` or empty. Set additional fields directly after construction.

### What this crate does not do

- **No validation**: `Keyword::new("")` succeeds. RFC 8621 keyword syntax rules are the
  server's responsibility.
- **No method handlers**: `Mailbox/get`, `Email/query`, etc. are implemented in
  `jmap-mail-server`.
- **No async**: this crate has no runtime dependency.
- **Partial `Email/get` responses**: `Email` requires the six metadata fields to be
  present on deserialization. Requests that use the `properties` argument to fetch only
  a subset of fields will fail to deserialize into `Email` if any required field is
  absent. Deserialize into `serde_json::Value` first for partial-property responses.

## Crate family

```
jmap-types                 shared wire primitives (RFC 8620)
    └── jmap-mail-types    ← this crate (RFC 8621 data types)
            ├── jmap-mail-server   method handlers, MailBackend trait
            └── jmap-mail-client   RFC 8621 HTTP client methods
```

## References

- [RFC 8621] — JMAP for Mail (normative)
- [RFC 8620] — JMAP Core (wire format foundation)
- [RFC 5322] — Internet Message Format (email structure, header fields)
- [RFC 5321] — SMTP (envelope address format used in `EmailSubmission`)
- [RFC 4314] — IMAP ACL Extension (basis for `MailboxRights`)

[RFC 8621]: https://www.rfc-editor.org/rfc/rfc8621
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[RFC 5322]: https://www.rfc-editor.org/rfc/rfc5322
[RFC 5321]: https://www.rfc-editor.org/rfc/rfc5321
[RFC 4314]: https://www.rfc-editor.org/rfc/rfc4314

## License

Licensed under either of [MIT License](LICENSE-MIT) or [Apache License, Version 2.0](LICENSE-APACHE) at your option.
