# jmap-sharing-types

Serde-annotated Rust types for JMAP Sharing ([RFC 9670]): `Principal` and
`ShareNotification`. Types only — no method handlers, no async, no network I/O.

## What

| Type | Module | Source |
|---|---|---|
| `Principal` | `principal` | RFC 9670 §2 |
| `PrincipalType` | `principal` | RFC 9670 §2 |
| `PrincipalFilterCondition` | `principal` | RFC 9670 §2.4.1 |
| `PrincipalProperty` | `backend` | RFC 9670 §2 — property enum for `Principal/get` |
| `ShareNotification` | `notification` | RFC 9670 §3 |
| `ChangedBy` | `notification` | RFC 9670 §3 |
| `ShareNotificationFilterCondition` | `notification` | RFC 9670 §3.4.1 |
| `ShareNotificationProperty` | `backend` | RFC 9670 §3 — property enum for `ShareNotification/get` |
| `PrincipalsCapability` | `capability` | RFC 9670 §1.5 — session-level capability object |
| `PrincipalsOwnerCapability` | `capability` | RFC 9670 §1.5 — owner-principal capability object |
| `JMAP_PRINCIPALS_URI` const (`"urn:ietf:params:jmap:principals"`) | `capability` | RFC 9670 §1.5 |
| `JMAP_PRINCIPALS_OWNER_URI` const (`"urn:ietf:params:jmap:principals:owner"`) | `capability` | RFC 9670 §1.5 |

## Filter extensibility

Filter types in this crate — `PrincipalFilterCondition`,
`ShareNotificationFilterCondition`, and the generic `Filter<T>` / `Operator`
re-exported from `jmap-types` — are **intentionally not extensible** via
vendor "extras" fields. A filter clause the server does not understand
silently breaks query correctness: the client gets the wrong set of records
back with no error signal. So these types deliberately have no `extra`
catch-all field.

Vendors who need to filter on custom fields have two options:

- **IETF-track (recommended).** Use `draft-ietf-jmap-metadata` (capability URI
  `urn:ietf:params:jmap:metadata`), which defines a `Metadata` / `Annotation`
  companion object keyed by `(relatedType, relatedId)` with capability-declared
  schema (`metadataTypes` / `maxDepth`) and a `Metadata/query` `textMatch`
  filter. This is the workspace's recommended path for vendor data that needs
  to be queryable; the implementation tracker is bd JMAP-06zp.
- **Pre-IETF escape.** If you cannot wait for the metadata draft, escape the
  filter tree to `serde_json::Value` or fork the `FilterCondition` types.
  See
  [`crate-jmap-calendars-types/PLAN.md`](../crate-jmap-calendars-types/PLAN.md)
  for the hybrid sloppy-value pattern.

This policy is part of the workspace extras-preservation policy documented in
the workspace [`AGENTS.md`](../AGENTS.md); the filter-algebra exclusion
decision is bd JMAP-lbdy.

## Usage

### Principal deserialization

```rust
use jmap_sharing_types::Principal;

let json = r#"{
    "id": "P2342fnddd20",
    "type": "individual",
    "name": "Joe Bloggs",
    "description": null,
    "email": "joe@example.com",
    "timeZone": "America/New_York",
    "capabilities": {},
    "accounts": null
}"#;

let p: Principal = serde_json::from_str(json).unwrap();
assert_eq!(p.name, "Joe Bloggs");
assert_eq!(p.email.as_deref(), Some("joe@example.com"));
// description and accounts are None (null on the wire)
assert!(p.description.is_none());
assert!(p.accounts.is_none());
```

### ShareNotification deserialization

```rust
use jmap_sharing_types::ShareNotification;

let json = r#"{
    "id": "notif1",
    "created": "2024-03-15T10:00:00Z",
    "changedBy": {
        "name": "Alice Smith",
        "email": "alice@example.com",
        "principalId": "P123"
    },
    "objectType": "Mailbox",
    "objectAccountId": "acc1",
    "objectId": "mb1",
    "oldRights": null,
    "newRights": { "mayReadItems": true, "mayAddItems": false },
    "name": "Team Inbox"
}"#;

let n: ShareNotification = serde_json::from_str(json).unwrap();
// oldRights is null — the user was newly granted access.
assert!(n.old_rights.is_none());
let new_rights = n.new_rights.as_ref().unwrap();
assert_eq!(new_rights.get("mayReadItems"), Some(&true));
```

## How it works

### camelCase serde

All structs carry `#[serde(rename_all = "camelCase")]`. Wire field names match
RFC 9670 exactly (`timeZone`, `changedBy`, `objectAccountId`, `principalId`,
etc.).

### Required-but-nullable fields

RFC 9670 marks several fields as required but nullable. These serialize as
`null` when their Rust value is `None` — they are never absent from the wire
JSON. Affected fields:

- `Principal`: `description`, `email`, `timeZone`, `accounts`
- `ChangedBy`: `email`, `principalId`
- `ShareNotification`: `oldRights`, `newRights`

None of these fields use `#[serde(skip_serializing_if)]`.

### PrincipalType open-endedness

`PrincipalType` is a Rust enum with an `Other(String)` catch-all variant.
Unknown future values round-trip unchanged. The RFC defines `"other"` as a
known value meaning "some other undefined Principal"; it maps to
`PrincipalType::Other("other".to_owned())`.

## Known Limitations

`Principal.accounts` is `Option<HashMap<Id, serde_json::Value>>`. The Account
object schema within this map varies per JMAP extension (each capability defines
its own `accountCapabilities` shape). This crate cannot enumerate all possible
schemas, so the values are left as untyped JSON. Callers needing typed account
capability access should deserialize the `Value` against their own struct.

## Crate family

```
jmap-types (RFC 8620 wire primitives)
    └── jmap-sharing-types  ← this crate
            ├── jmap-sharing-server (method handlers)
            └── jmap-sharing-client (client extension trait)
```

## References

- **[RFC 9670]** — JMAP Sharing (normative for Principal, ShareNotification,
  rights model, and filter conditions)
- **[RFC 8620]** — JMAP Core (request format, Id, UTCDate, State, Filter)

[RFC 9670]: https://www.rfc-editor.org/rfc/rfc9670
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620

## License

MIT OR Apache-2.0
