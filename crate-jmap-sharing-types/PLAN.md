# jmap-sharing-types — Implementation Plan

RFC 9670 (JMAP Sharing) data types. Types only — no method handlers, no
async, no network I/O. This crate sits between `jmap-types` (shared JMAP
base primitives) and `jmap-sharing-server` / `jmap-sharing-client`.

## Crate Family Position

```
jmap-types (RFC 8620 wire primitives)
    └── jmap-sharing-types  ← this crate
            ├── jmap-sharing-server (method handlers)
            └── jmap-sharing-client (extension trait)
```

Domain crates that add `shareWith` to their own types (`jmap-mail-types`,
future calendar/contacts/filenode crates) do NOT depend on this crate.
They use `jmap_types::Id` as the map key and define their own rights
structs. This crate only defines `Principal` and `ShareNotification`.

## What This Crate Covers

Each object type maps to one source module. The RFC 9670 section is the
normative reference for field names, types, and serialization.

| Module | Type(s) | RFC 9670 section |
|---|---|---|
 | `principal.rs` | `Principal`, `PrincipalType`, `PrincipalFilterCondition` | §2 |
 | `notification.rs` | `ShareNotification`, `ChangedBy`, `ShareNotificationFilterCondition` | §3 |
 | `capability.rs` | `PrincipalsCapability`, `PrincipalsOwnerCapability` | §1.5 |

## What Is Out of Scope

- Method handlers (`Principal/get`, `Principal/query`, etc.) — live in
  `jmap-sharing-server`
- The `isSubscribed`, `myRights`, `shareWith` properties that appear on
  shareable domain types (Mailbox, Calendar, etc.) — those are defined on
  the domain types themselves, not here
- Transport and network I/O — no tokio, no reqwest
- Principal management integration with directory services — backend concern

## Full Type Reference

### `Principal` (RFC 9670 §2)

All fields are as specified in §2. Fields that the spec marks `server-set`
are still present as regular fields; enforcement of immutability is a
handler concern, not a type concern.

| Field | Wire name | Rust type | Notes |
|---|---|---|---|
| `id` | `id` | `Id` | server-set, immutable |
| `type_` | `type` | `PrincipalType` | see enum below |
| `name` | `name` | `String` | human-readable display name |
| `description` | `description` | `Option<String>` | null if no description |
| `email` | `email` | `Option<String>` | must be addr-spec (RFC 5322 §3.4.1) |
| `time_zone` | `timeZone` | `Option<String>` | IANA time zone name |
| `capabilities` | `capabilities` | `HashMap<String, serde_json::Value>` | capability URI → domain-specific object |
| `accounts` | `accounts` | `Option<HashMap<Id, serde_json::Value>>` | Account id → Account object; null if none |

The `capabilities` and `accounts` fields use `serde_json::Value` because
their schemas are defined by other specifications (capability documents and
RFC 8620 respectively). This crate does not re-define the Account type.

`accounts` is `Option<HashMap<…>>` to represent the nullable map: null
means the principal owns no accounts accessible to the caller.

### `PrincipalType` (RFC 9670 §2)

String enum with a catch-all `Other(String)` variant, identical in pattern
to `MailboxRole` in `jmap-mail-types`. The spec defines five known values
and requires clients to handle unknown values gracefully.

| Wire value | Rust variant |
|---|---|
| `"individual"` | `Individual` |
| `"group"` | `Group` |
| `"resource"` | `Resource` |
| `"location"` | `Location` |
| `"other"` | `Other` |

Uses a manual serde implementation (not `#[serde(rename_all)]`) to support
`Other(String)`. Any unrecognized string round-trips through `Other(s)`.

### `ShareNotification` (RFC 9670 §3)

All fields are server-set and immutable. The handler enforces this; the type
does not.

| Field | Wire name | Rust type | Notes |
|---|---|---|---|
| `id` | `id` | `Id` | server-set, immutable |
| `created` | `created` | `UTCDate` | server-set, immutable |
| `changed_by` | `changedBy` | `ChangedBy` | see struct below |
| `object_type` | `objectType` | `String` | IANA JMAP Data Types registry name |
| `object_account_id` | `objectAccountId` | `Id` | account where the object lives |
| `object_id` | `objectId` | `Id` | id of the shared object |
| `old_rights` | `oldRights` | `Option<HashMap<String, bool>>` | rights before; null if newly added |
| `new_rights` | `newRights` | `Option<HashMap<String, bool>>` | rights after; null if access removed |
| `name` | `name` | `String` | name of the object at notification time |

### `ChangedBy` (RFC 9670 §3 — the "Entity" object)

The RFC calls this an "Entity" but it is an anonymous inline object in the
spec. Named `ChangedBy` here for clarity.

| Field | Wire name | Rust type | Notes |
|---|---|---|---|
| `name` | `name` | `String` | display name of who made the change |
| `email` | `email` | `Option<String>` | email of changer, or null |
| `principal_id` | `principalId` | `Option<Id>` | corresponding Principal id, or null |

### `PrincipalFilterCondition` (RFC 9670 §2.4.1)

Used in `Principal/query`. All fields optional.

| Field | Wire name | Rust type | Notes |
|---|---|---|---|
| `account_ids` | `accountIds` | `Option<Vec<String>>` | match if any id is in principal's accounts |
| `email` | `email` | `Option<String>` | substring match |
| `name` | `name` | `Option<String>` | substring match |
| `text` | `text` | `Option<String>` | substring match in name, email, or description |
| `type_` | `type` | `Option<PrincipalType>` | exact match |
| `time_zone` | `timeZone` | `Option<String>` | exact match |

### `ShareNotificationFilterCondition` (RFC 9670 §3.4.1)

Used in `ShareNotification/query`. All fields optional.

| Field | Wire name | Rust type | Notes |
|---|---|---|---|
| `after` | `after` | `Option<UTCDate>` | created must be on or after this |
| `before` | `before` | `Option<UTCDate>` | created must be before this |
| `object_type` | `objectType` | `Option<String>` | exact match |
| `object_account_id` | `objectAccountId` | `Option<Id>` | exact match |

Sorting: the `created` field MUST be supported as a sort property (RFC 9670
§3.4.2). No dedicated `ShareNotificationComparator` type is required —
the generic `Comparator` from `jmap-types` is used with `"created"` as the
property name.

### Capability structs (RFC 9670 §1.5)

Two capability structs are needed for the Session object:

`PrincipalsCapability` — value of `urn:ietf:params:jmap:principals` in an
Account's `accountCapabilities`. Contains:
- `current_user_principal_id: Option<Id>` — the id of the Principal
  corresponding to the requesting user, or null.

`PrincipalsOwnerCapability` — value of
`urn:ietf:params:jmap:principals:owner` in an Account's
`accountCapabilities`. Present only on Accounts that are owned by a
Principal. Contains:
- `account_id_for_principal: Id` — id of the Account that holds the
  corresponding Principal object
- `principal_id: Id` — id of the Principal that owns this Account

`urn:ietf:params:jmap:principals:owner` does NOT appear in the Session-level
`capabilities` object — only in `accountCapabilities`. It is NOT an
independent capability; its presence is implied by
`urn:ietf:params:jmap:principals` being in the Session capabilities.

## Key Design Decisions

### PrincipalId is just `jmap_types::Id` — no newtype

Creating a `PrincipalId(Id)` newtype would force every domain crate that
uses `shareWith` maps to depend on this crate. RFC 9670 §4 specifies that
`shareWith` uses Principal ids as keys, but the key type is simply `Id` on
the wire. Domain crates use `jmap_types::Id` directly.

Alternative considered: expose `PrincipalId` as a type alias
(`type PrincipalId = Id`). Rejected because a type alias adds no type safety
and would still need to be imported. The current approach (plain `Id`) is
simpler and keeps domain crates independent.

### `capabilities` and `accounts` use `serde_json::Value`

The `Principal.capabilities` field maps capability URIs to objects whose
schema is defined by each capability's own specification. This crate cannot
know all possible capability object shapes. Using `serde_json::Value` allows
the type to be complete (deserializes without loss) while deferring
interpretation to the consumer.

`Principal.accounts` is similarly open-ended — it holds Account objects
whose full schema is defined by RFC 8620 §2 and extends with per-account
capability data. The Account type lives in `jmap-types`; including a
`HashMap<Id, Account>` would require importing `jmap-types`'s Account type.
Using `serde_json::Value` avoids a circular or awkward dependency.

Alternative considered: define a minimal `PrincipalAccount` struct with only
the fields this crate needs. Rejected because the account shape varies per
deployment and partial deserialization would silently drop fields.

### `PrincipalType` uses manual serde for `Other(String)` catch-all

RFC 9670 §2 requires implementations to handle unknown `type` values
gracefully. This is the same pattern used by `MailboxRole` in
`jmap-mail-types`. `#[serde(rename_all = "lowercase")]` alone cannot produce
a catch-all; a manual `Deserialize` impl is required.

### Filter types use `Option` fields, not nested `FilterOperator`

`PrincipalFilterCondition` and `ShareNotificationFilterCondition` are leaf
conditions (all conditions AND'd together). RFC 9670 does not define a
compound filter operator for these types, unlike RFC 8620's generic
`Filter<T>` with `FilterOperator`. These are concrete structs, not wrapped
in `Filter<T>`.

Alternative considered: using the generic `Filter<PrincipalFilterCondition>`
from `jmap-types`. Accepted as the right approach for the query handler
(which may use filter operators from RFC 8620), but the condition struct
itself is concrete. The query handler in `jmap-sharing-server` will use
`Filter<PrincipalFilterCondition>` for the method request, accepting the
RFC 8620 filter compound syntax.

## Module Layout

```
src/
  lib.rs             re-exports of all public types
  principal.rs       Principal, PrincipalType
  notification.rs    ShareNotification, ChangedBy
  query.rs           PrincipalFilterCondition, ShareNotificationFilterCondition
  capability.rs      PrincipalsCapability, PrincipalsOwnerCapability
```

## Test Oracle Strategy

Tests must use independent oracles — never derive expected values from the
code under test. Acceptable sources:

1. Literal JSON from RFC 9670 examples (§4.1 contains a full Principal
   response example — copy-paste from the spec).
2. Hand-written JSON fixtures constructed directly from RFC 9670 field
   descriptions (committed in `tests/fixtures/`).
3. Known wire values verified against the RFC text.

All tests are `#[test]` (no tokio). Roundtrip tests (`serialize →
deserialize`) verify serde consistency but are not a substitute for
spec-grounded oracle tests.

Specific test cases:

- `Principal` roundtrip with all fields populated (using RFC 9670 §4.1
  example: "Joe Bloggs", "P2342fnddd20", `"type": "individual"`, etc.)
- `Principal` with `accounts: null` and `capabilities: {}` — must not
  error
- `PrincipalType` deserialization of each known value + an unknown string
  value (must produce `Other("unknown-value")` not an error)
- `ShareNotification` with `old_rights: null` (new share) and
  `new_rights: null` (access removed)
- `PrincipalFilterCondition` roundtrip with each field individually set
- `ShareNotificationFilterCondition` with `after` and `before` set
- `PrincipalsOwnerCapability` roundtrip against the session object example
  in RFC 9670 §4.1

## Spec References

- `~/PROJECT/jmap-chat-spec/references/rfc9670.txt` — JMAP Sharing (normative)
- `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-mail-sharing-00.txt` —
  extends RFC 8621 Mailbox with `shareWith`/`mayShare` (context only; the
  Mailbox extension types live in `jmap-mail-types`, not here)
- `~/PROJECT/jmap-chat-spec/references/rfc8620.txt` — base protocol (for
  Account, Session, Id, UTCDate, State)

## Dependencies

```toml
jmap-types = { path = "../crate-jmap-types" }
serde      = { version = "1", features = ["derive"] }
serde_json = "1"
# No tokio, no async, no network deps
```
