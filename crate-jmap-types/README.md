# jmap-types

Shared JMAP wire types for the `jmap-*` crate family.

Implements the core data structures defined in [RFC 8620] (JMAP Core): identifiers,
timestamps, state tokens, method-level errors, request/response envelopes, and
result references. All types serialize to and from the exact JSON wire format the
spec requires.

## What it is

| Type | RFC | Description |
|------|-----|-------------|
| `Id` | §1.2 | Opaque server-assigned record identifier |
| `UTCDate` | §1.4 | RFC 3339 UTC timestamp string |
| `State` | §1.2 | Opaque server state token |
| `JmapError` | §3.6.2 | Method-level error with typed constructors |
| `Invocation` | §3.2 | `(method_name, arguments, call_id)` 3-tuple |
| `JmapRequest` | §3.3 | HTTP request envelope |
| `JmapResponse` | §3.4 | HTTP response envelope |
| `ResultReference` | §9 | Reference to a prior method's result |
| `Argument<T>` | §9 | A method argument that is either a value or a `ResultReference` |

The `query` module also re-exports the generic filter algebra used across the
crate family: `Filter<T>`, `FilterOperator<T>`, and `Operator` (RFC 8620 §5.5).

## Filter extensibility

Filter and comparator types in this crate — the generic `Filter<T>`,
`FilterOperator<T>`, and `Operator` — are **intentionally not extensible** via
vendor "extras" fields. A filter clause the server does not understand silently
breaks query correctness: the client gets the wrong set of records back with no
error signal. So these types deliberately have no `extra` catch-all field, and
`Operator` has no `Other(String)` variant. The same exclusion applies to
every per-object `FilterCondition` / `Comparator` / `ComparatorProperty` type
in the downstream `jmap-*-types` crates.

Vendors who need to filter on custom fields have two options:

- **IETF-track (recommended).** Use the JMAP Object Metadata extension
  (`draft-ietf-jmap-metadata`, capability URI `urn:ietf:params:jmap:metadata`),
  which defines a `Metadata` / `Annotation` companion object keyed by
  `(relatedType, relatedId)` with capability-declared schema (`metadataTypes`
  / `maxDepth`) and a `Metadata/query` `textMatch` filter. This is the
  workspace's recommended path for vendor data that needs to be queryable.
  Implemented in [`jmap-metadata-types`](../crate-jmap-metadata-types),
  [`jmap-metadata-server`](../crate-jmap-metadata-server), and
  [`jmap-metadata-client`](../crate-jmap-metadata-client) (bd JMAP-06zp).
- **Pre-IETF escape.** If you cannot wait for the metadata draft, escape the
  filter tree to `serde_json::Value` or fork the per-crate `FilterCondition`
  type. See
  [`crate-jmap-calendars-types/PLAN.md`](../crate-jmap-calendars-types/PLAN.md)
  for the hybrid sloppy-value pattern.

This policy is part of the workspace extras-preservation policy documented in
the workspace [`AGENTS.md`](../AGENTS.md); the filter-algebra exclusion
decision is bd JMAP-lbdy.

## What it's for

Client crates and server crates both need these types, but neither should pull in
the other's dependencies. `jmap-types` has exactly three dependencies — `serde`,
`serde_json`, and `thiserror` — and no async runtime, no HTTP framework, and no
application logic. Any crate in the `jmap-*` family can depend on it without cost.

## How to use

Add to `Cargo.toml`:

```toml
[dependencies]
jmap-types = "0.1"
```

### Identifiers

```rust
use jmap_types::{Id, UTCDate, State};

// Construct from any string source
let id = Id::from("abc123");
let date = UTCDate::from("2014-10-30T06:12:00Z");
let state = State::from("75128aab4b1b");

// Use as a string reference without allocating
println!("{}", id);          // "abc123"
let s: &str = id.as_ref();  // borrow the inner string

// Consume to get the inner String
let raw: String = id.into_inner();
```

### Building a request

```rust
use jmap_types::JmapRequest;
use serde_json::json;

// `JmapRequest::new` is required because the struct is `#[non_exhaustive]`
// and cannot be built with a struct literal outside this crate.
let req = JmapRequest::new(
    vec![
        "urn:ietf:params:jmap:core".into(),
        "urn:ietf:params:jmap:mail".into(),
    ],
    vec![
        ("Email/get".into(), json!({"accountId": "u1", "ids": ["m1"]}), "c1".into()),
    ],
    None,
);

let json = serde_json::to_string(&req)?;
// {"using":[...],"methodCalls":[["Email/get",{"accountId":"u1","ids":["m1"]},"c1"]]}
```

### Parsing a response

```rust
use jmap_types::JmapResponse;

let raw = r#"{
  "methodResponses": [["Email/get", {"list": []}, "c1"]],
  "sessionState": "75128aab4b1b"
}"#;

let resp: JmapResponse = serde_json::from_str(raw)?;
println!("{}", resp.session_state); // "75128aab4b1b"
```

### Method-level errors

```rust
use jmap_types::JmapError;

// Emit a typed error into a methodResponses array
let err = JmapError::invalid_arguments("ids must not be empty");
let err = JmapError::unknown_method();
let err = JmapError::server_fail("unexpected database error");

// alreadyExists requires the existing record's Id (RFC 8620 §5.4 MUST)
use jmap_types::Id;
let err = JmapError::already_exists(Id::from("existing-record-id"));

// Serialize for the wire — key is "type", not "error_type"
// {"type":"invalidArguments","description":"ids must not be empty"}
let json = serde_json::to_string(&err)?;
```

### Result references

```rust
use jmap_types::{Argument, ResultReference};

// A plain value argument
let arg: Argument<Vec<jmap_types::Id>> = Argument::Value(vec![Id::from("m1")]);

// A result reference — resolves at dispatch time
let rr = ResultReference::new("c0", "Email/query", "/ids");
let arg: Argument<Vec<jmap_types::Id>> = Argument::Ref(rr);
```

## How it works

### Wire format

All types use `serde` with the JSON field names and structure RFC 8620 mandates:

- `JmapRequest.method_calls` serializes as `"methodCalls"` (camelCase)
- `JmapResponse.session_state` serializes as `"sessionState"`
- `JmapError.error_type` serializes as `"type"` (the RFC field name)
- `JmapError.existing_id` serializes as `"existingId"` (present only for `alreadyExists`)
- `Invocation` is a type alias for `(String, serde_json::Value, String)`, which
  serde serializes as a 3-element JSON array — exactly the RFC 8620 §3.2 format

### `Argument<T>` and the sealed trait

`Argument<T>` uses `#[serde(untagged)]` to deserialize either as the value type `T`
or as a `ResultReference` (a JSON object). This works correctly only if `T` cannot
itself deserialize from a JSON object — otherwise serde would match `T` first and
silently swallow any `ResultReference` payload.

To prevent this, `T` is constrained to a sealed set of types that are guaranteed
to not deserialize from objects: `String`, `Vec<String>`, `Id`, `Vec<Id>`, `u32`,
`u64`, `bool`. `serde_json::Value` is deliberately excluded — `Argument<Value>`
fails to compile.

The sealed trait lives in a private module and cannot be implemented outside this
crate. To add a new type, open a PR and verify the invariant holds.

### `#[non_exhaustive]` and field privacy

Public structs are `#[non_exhaustive]` so that adding fields in future versions is
not a breaking change. The `Id`, `UTCDate`, and `State` newtypes also carry
`#[non_exhaustive]` specifically to prevent callers from pattern-matching the inner
field (e.g. `let Id(s) = id;`), which would otherwise lock in the single-field
layout as a semver guarantee. Use `as_ref()` or `into_inner()` instead.

### What this crate does not do

- **No validation**: `Id::from("")` succeeds. RFC 8620 requires non-empty Ids, but
  enforcement belongs in the consumer (e.g. `jmap-server`), not in the wire type.
- **No dispatch**: method routing, argument resolution, and `#`-prefixed key
  handling are the dispatcher's job.
- **No async**: this crate has no runtime dependency.

## Crate family

```
jmap-types          ← this crate
    ├── jmap-server     dispatcher, axum, tokio
    ├── jmap-mail-types RFC 8621 data types
    └── jmap-chat-types Chat extension data types
```

## Gotchas

- **No field validation.** `Id::from("")` succeeds. `UTCDate::from("not-a-date")` succeeds. RFC 8620 field constraints (non-empty Ids, valid date formats) are enforced by consumers such as `jmap-server`, not here.
- **`Argument<T>` sealed type set.** The `Argument<T>` type (which holds either a plain value or a `ResultReference`) is constrained to a sealed set of inner types (`String`, `Vec<String>`, `Id`, `Vec<Id>`, `u32`, `u64`, `bool`). `serde_json::Value` is intentionally excluded. To add a new type, a PR to this crate is required.
- **`JmapError` method-level only.** `JmapError` covers RFC 8620 §3.6.2 method-level errors. Request-level errors (§3.6.1, returned as HTTP 400/500 with an RFC 7807 body) are `RequestError` in `jmap-server`, not here.

## References

- [RFC 8620] — JMAP Core (normative)
- [RFC 7807] — Problem Details for HTTP APIs (request-level errors, §3.6.1)
- [RFC 6901] — JSON Pointer (used in `ResultReference.path`)

[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[RFC 7807]: https://www.rfc-editor.org/rfc/rfc7807
[RFC 6901]: https://www.rfc-editor.org/rfc/rfc6901
