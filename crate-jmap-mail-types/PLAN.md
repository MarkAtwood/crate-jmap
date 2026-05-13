# jmap-mail-types Plan

RFC 8621 (JMAP for Mail) data types.  Types only — no method handlers, no
async, no network I/O.  This crate sits between `jmap-types` (shared JMAP base
primitives) and `jmap-mail-server` (method handlers).

## Crate Family Position

```
jmap-types (RFC 8620 wire primitives)
    └── jmap-mail-types  ← this crate
            └── jmap-mail-server (method handlers)
```

## What This Crate Covers

Each object type maps to one source module.  The corresponding RFC 8621 section
is the normative reference for field names, types, and serialization:

| Module | Type(s) | RFC 8621 section |
|---|---|---|
| `mailbox.rs` | `Mailbox`, `MailboxRole`, `MailboxRights` | §2 |
| `thread.rs` | `Thread` | §3 |
| `email.rs` | `Email`, `EmailAddress`, `EmailBodyPart`, `EmailBodyValue` | §4 |
| `identity.rs` | `Identity` | §5 |
| `submission.rs` | `EmailSubmission`, `Envelope`, `Address`, `UndoStatus`, `EmailSubmissionFilterCondition` | §7 |
| `snippet.rs` | `SearchSnippet` | §4.5 |
| `vacation.rs` | `VacationResponse` | §8 |
| `query.rs` | `EmailFilterCondition`, `EmailFilter`, `EmailComparator` | §4.4 |
| `keyword.rs` | keyword constants | §4.1.1 |

Generic query types (`Filter<T>`, `FilterOperator<T>`, `Operator`) live in
`jmap-types::query` because they are defined by the base protocol (RFC 8620 §5.5),
not RFC 8621.

## What Is Out of Scope

- Method handlers (`Email/get`, `Email/query`, etc.) — those live in `jmap-mail-server`
- MIME parsing and reassembly — consumer responsibility
- Transport and network I/O — no tokio, no reqwest
- Partial PATCH semantics — `jmap-mail-server` applies patches; this crate holds the types

## Key Design Decisions

### MailboxRole custom serde
`MailboxRole` uses a manual serde implementation (not `#[serde(rename_all)]`) to
support an `Other(String)` catch-all.  RFC 8621 §2.7 lists known roles but
requires clients to accept unknown role values gracefully.

### Email — all fields are Option
`Email` uses `Option` for almost every field because RFC 8621 §4.5 allows
partial responses (clients request only the fields they need via `properties`).
A field absent from the server response must not fail deserialization.

### Filter union — untagged enum with Operator first
`Filter<T>` uses `#[serde(untagged)]` with the `Operator` variant listed before
`Condition`.  Serde untagged tries variants in declaration order; `FilterOperator`
requires an `"operator"` key and fails fast when absent, letting the deserializer
fall through to `Condition(T)`.  `FilterCondition` must NOT use
`#[serde(deny_unknown_fields)]` because untagged deserialization does not work
correctly with that attribute.

### EmailFilterCondition — header validation
The `header` field of `EmailFilterCondition` must have 1 or 2 elements per RFC
8621 §4.4.1.  Validation happens in a custom `deserialize_with` function rather
than at method-call time; invalid input is rejected at the wire boundary.

### EmailComparator — isAscending default and skip
`isAscending` defaults to `true` (RFC 8620 §5.5) and is omitted from
serialized output when `true`.  This keeps the wire representation minimal and
matches the RFC examples.

### VacationResponse — singleton id
The RFC says the `id` is always `"singleton"`, but the field is still a regular
`Id` in the struct.  Enforcement of the singleton constraint is a server concern.

### Keyword constants — &str not enum
Keywords are `pub const &str` values in `mod keyword` rather than an enum.
This avoids allocation overhead when used as `HashMap<String, bool>` keys and
allows unknown keywords to pass through without error.

## Test Oracle Strategy

Tests must use independent oracles — never derive expected values from the code
under test.  Acceptable sources:

1. Hand-written JSON fixtures constructed directly from RFC 8621 field
   descriptions (committed in `tests/fixtures/`).
2. Literal JSON from RFC 8621 examples (copy-pasted from the RFC text).
3. Known wire values verified against the RFC text.

All tests are `#[test]` (no tokio).  Roundtrip tests (`serialize → deserialize`)
verify serde consistency but are not a substitute for spec-grounded oracle tests.

## Spec References

- `~/PROJECT/jmap-chat-spec/references/rfc8621.txt` — JMAP for Mail (normative)
- `~/PROJECT/jmap-chat-spec/references/rfc8620.txt` — JMAP base protocol (for
  Filter, Comparator, and session types)

## Dependencies

- `jmap-types` (path dep) — `Id`, `UTCDate`, `Date`, `State`, `Filter`,
  `FilterOperator`, `Operator`
- `serde` + `serde_json` — serialization
- No tokio, no async, no network deps

## Type-design constraints

### Extras-preservation policy (JMAP-lbdy)

Every public `Deserialize` struct that appears on the JMAP wire carries an
`extra` field per the workspace extras-preservation policy (see workspace
`AGENTS.md`):

```rust
#[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
pub extra: serde_json::Map<String, serde_json::Value>,
```

This preserves vendor / site / private-extension fields across
deserialize/serialize round-trip. Wire format is byte-identical when extras
are empty.

In scope in this crate (each has at least one round-trip preservation test):

- `EmailAddress`, `EmailAddressGroup`, `EmailHeader`, `EmailBodyValue`,
  `EmailBodyPart`, `Email` (`email.rs`).
- `MailboxRights`, `Mailbox` (`mailbox.rs`).
- `Identity` (`identity.rs`).
- `SearchSnippet` (`snippet.rs`).
- `VacationResponse` (`vacation.rs`).
- `SieveScript` (`sieve.rs`, `#[cfg(feature="sieve")]`).
- `Thread` (`thread.rs`).
- `Address`, `Envelope`, `DeliveryStatus`, `EmailSubmission`
  (`submission.rs`).
- `Disposition`, `Mdn`, `MdnSendRequest`, `MdnSendResponse`,
  `MdnParseRequest`, `MdnParseResponse` (`mdn.rs`, `#[cfg(feature="mdn")]`).

Out of scope (explicitly excluded by the workspace policy):

- Filter and comparator algebra types (`EmailFilterCondition`,
  `EmailComparator`, `MailboxFilterCondition`,
  `EmailSubmissionFilterCondition`) — silent-drop of an unknown filter
  clause is a server-side query-correctness bug, not a round-trip
  preservation problem. See workspace AGENTS.md "Filter algebra and
  control enums are explicitly EXCLUDED" for the full rationale.
- Capability objects (`SieveCapability`, `SieveAccountCapability`) —
  capability objects live in the Session response and follow Session's
  own evolution rules, not the data-object preservation policy.
- Newtypes (`Keyword`) — newtypes wrapping a single value have no
  field-shaped extension surface.
- String enums (`MailboxRole`, `Delivered`, `Displayed`, `UndoStatus`,
  `ActionMode`, `SendingMode`, `DispositionType`,
  `EmailSubmissionState`) — these are result/control enums; the result
  enums tracked by JMAP-lbdy receive `Unknown(String)` variants via the
  separate enum-side propagation track, not the struct `extra` field.

### New-type rule

Any new public `Deserialize` struct added to this crate that appears on
the JMAP wire MUST include the `extra` field from day one with the
documented serde attributes and at least one round-trip preservation
test. Cookie-cutter sibling crates (`jmap-chat-types`,
`jmap-calendars-types`, `jmap-tasks-types`, `jmap-contacts-types`,
`jmap-filenode-types`, `jmap-sharing-types`) mirror this rule.

## JMAP Object Metadata `relatedType` declarations

The JMAP Object Metadata extension
([draft-ietf-jmap-metadata-01](https://datatracker.ietf.org/doc/draft-ietf-jmap-metadata/))
defines a companion `Metadata` object keyed by `(relatedType, relatedId)`
that attaches vendor-defined `Annotation`s — and, for some types, IMAP
`ImapMetadata` or WebDAV `WebDavMetadata` records — to objects defined
elsewhere in the workspace.

The data types in this crate that are valid `relatedType` values:

| relatedType | Flavours supported by spec |
|---|---|
| `Mailbox` | `Annotation`; `ImapMetadata` (draft §2.1.2 MUST) |
| `Email` | `Annotation` |
| `Thread` | `Annotation` |
| `EmailSubmission` | `Annotation` |
| `Identity` | `Annotation` |

Servers that declare `urn:ietf:params:jmap:metadata` MAY restrict the
set of supported `relatedType`s via the capability's `dataTypes`
property. Backends that index `ImapMetadata` MUST enforce the
`relatedType == "Mailbox"` constraint per draft §2.1.2.

Implementation crates: `jmap-metadata-types`, `jmap-metadata-server`,
`jmap-metadata-client` (bd JMAP-06zp).
