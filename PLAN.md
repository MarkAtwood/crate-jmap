# jmap-types — Implementation Plan

Shared JMAP wire types for the jmap-* crate family.
Depends only on serde, serde_json, and thiserror — no tokio, no axum, no async.

## Crate Family Position

```
jmap-types  ← this crate
    ├── jmap-server          adds dispatcher, axum, tokio
    ├── jmap-mail-types      adds RFC 8621 data types
    ├── jmap-chat-types      adds Chat extension data types
    └── (future extensions)
```

## What This Crate Is

The type foundation for the entire jmap-* family. Any crate that needs to speak
JMAP wire format depends on this crate. Client-only crates can depend on it
directly without pulling in server-side deps (tokio, axum).

## What This Crate Is Not

- Not a dispatcher or server framework (that is `jmap-server`)
- Not opinionated about methods, capabilities, or extensions
- Not async

## Source Material

Types currently live in kith-core and are sketched in `crate-jmap-server/PLAN.md`.
This crate extracts them so all family members share one definition.

| Item | Current location |
|---|---|
| `JmapRequest`, `JmapResponse`, `Invocation` | `~/PROJECT/kith/crates/kith-core/src/jmap.rs` |
| `JmapError` | `~/PROJECT/kith/crates/kith-core/src/error.rs` |
| `ResultReference`, `Argument<T>` | `~/PROJECT/kith/crates/kith-core/src/resultref.rs` |
| `Id`, `UTCDate`, `State` | `~/PROJECT/kith/crates/kith-core/src/jmap.rs` |

## Dependencies

```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
```

No other dependencies.

## Planned Public API

```rust
// Core identifiers (RFC 8620 §1.2)
pub struct Id(String);       // opaque server-assigned ID
pub struct UTCDate(String);  // ISO 8601 UTC datetime string
pub struct State(String);    // opaque state token

// Errors (RFC 8620 §7.1)
pub struct JmapError { pub error_type: String, pub description: Option<String> }
impl JmapError {
    pub fn invalid_arguments(desc) -> Self
    pub fn forbidden_method() -> Self
    pub fn not_found() -> Self
    pub fn account_not_found() -> Self
    pub fn server_fail(desc) -> Self
    pub fn cannot_calculate_changes() -> Self
    pub fn state_mismatch() -> Self
    pub fn unknown_capability(cap) -> Self
    pub fn request_too_large(desc) -> Self
    pub fn unknown_method() -> Self
    pub fn too_large() -> Self
}

// Wire types (RFC 8620 §3)
pub type Invocation = (String, serde_json::Value, String);
pub struct JmapRequest  { pub using: Vec<String>, pub method_calls: Vec<Invocation> }
pub struct JmapResponse { pub method_responses: Vec<Invocation>, pub session_state: String,
                          pub created_ids: Option<HashMap<String, String>> }

// ResultReference (RFC 8620 §9)
pub struct ResultReference { pub result_of: String, pub name: String, pub path: String }
pub enum Argument<T: Sealed> { Value(T), Ref(ResultReference) }
```

## Module Layout

```
src/
  lib.rs        re-exports
  id.rs         Id, UTCDate, State
  error.rs      JmapError and constructors
  wire.rs       JmapRequest, JmapResponse, Invocation
  resultref.rs  ResultReference, Argument<T>
```

## Spec Reference

```
~/PROJECT/jmap-chat-spec/references/rfc8620.txt   ← normative
```

## Test Strategy

- Serde round-trips against hand-written JSON derived from RFC 8620 examples
- `JmapError` type strings match the exact strings in RFC 8620 §7.1
- Tests must use fixtures as independent oracle — not the code under test itself
