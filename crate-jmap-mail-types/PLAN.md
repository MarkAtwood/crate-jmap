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
