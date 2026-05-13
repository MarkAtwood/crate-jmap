# jmap-cid-types — Implementation Plan

draft-atwood-jmap-cid-00 wire-format types for the `jmap-*` crate
family. Pure types — no method handlers, no async, no network I/O.

## Crate family position

```
jmap-types
    └── jmap-cid-types  ← this crate
```

Future consumers (under follow-up beads in epic bd:JMAP-v9py):

- `jmap-base-client` — Blob upload response gains a typed `sha256`
  field; `Session::supports_cid()` is added alongside the existing
  capability accessors.
- A future `jmap-filenode-types` revision — FileNode object gains
  a typed `sha256: Option<Sha256>` property when both CID and
  FileNode capabilities are advertised.

## What this crate is

The wire-format types defined by draft-atwood-jmap-cid-00:

1. **`CidCapability`** — the value object of the
   `urn:ietf:params:jmap:cid` capability in the JMAP Session
   resource. Currently empty per the draft (§3) but
   `#[non_exhaustive]` per workspace policy to allow future
   capability fields without a breaking change.
2. **`Sha256`** — a typed wrapper around the 64-character lowercase
   hex `sha256-value` from the draft's ABNF (§2). Parse-time
   validation enforces the ABNF; the wire format is the same hex
   string round-trip preserved.

## What this crate is not

- Not the Blob upload response binding (lives in `jmap-base-client`).
- Not a FileNode `sha256` field definition (lives in a future
  `jmap-filenode-types` revision).
- Not a SHA-256 hash *implementation* — content-hash computation is
  a server-side concern. This crate carries the wire shape only.
- Not a base64 digest binding for RFC 9404 `Blob/get`
  `digest:sha-256` requests. CID and BLOBEXT are deliberately
  separate mechanisms with different encodings (lowercase hex vs.
  base64) and different access patterns (unconditional at upload
  vs. on-demand via `Blob/get`); see draft §2.3.

## Dependencies

```toml
jmap-types = { workspace = true }
serde      = { workspace = true }
serde_json = { workspace = true }
```

`jmap-types` is included per the parent epic's acceptance criteria
(bd:JMAP-v9py.11). The eventual `Sha256` shape may or may not
reference `jmap-types` primitives directly; the dep is reserved in
the skeleton so D.2 can land without a `Cargo.toml` churn commit.
If D.2 ships without referencing `jmap-types` and `cargo udeps`
flags it, the cleanup is a one-line edit and the workspace can
decide whether to drop the dep then.

## Public API (skeleton state)

`src/lib.rs` currently carries only the crate-level doc-comment and
`#[forbid(unsafe_code)]`. No types are exported yet.

The follow-up beads will add:

- `pub struct CidCapability { /* empty, #[non_exhaustive] */ }`
- `pub struct Sha256(String);` with `FromStr` / `TryFrom<String>`
  parse-time ABNF validation plus `Display` / `Serialize` /
  `Deserialize` impls. Wire format = the inner 64-char lowercase
  hex string.

Module layout will mirror `jmap-metadata-types`:
`capability.rs` for the capability marker, a per-type module for
`Sha256`, and `lib.rs` re-exports.

## Spec reference

```
~/PROJECT/jmap-chat-spec/draft-atwood-jmap-cid-00.md   ← normative
```

Key sections:

- §2 (Conventions) — defines the `sha256-value` ABNF rule
  (`64( %x30-39 / %x61-66 )`, 64 lowercase hex digits).
- §3 (Capability) — `urn:ietf:params:jmap:cid`, empty value object.
- §4 (Blob Upload Response Extension) — `sha256` field shape.
- §5 (FileNode Extension) — `sha256` property on FileNode objects.

## Reference implementations to mirror

This crate models the `jmap-types::Id` newtype style for the
`Sha256` typed shape: a `#[serde(transparent)]` newtype around
`String` with parse-time validation in a `TryFrom<String>` or
`FromStr` impl. See `crate-jmap-types/src/id.rs` for the canonical
pattern.

For the capability marker, see `crate-jmap-metadata-types/src/capability.rs`
(`MetadataCapability`) and `crate-jmap-types/src/capability.rs`
(`CoreCapability`) for the empty-object-with-`#[non_exhaustive]`
shape.

## Round-trip test policy

Each follow-up bead that adds a type to this crate ships at least
one round-trip serde test using hand-written example JSON from the
draft (the upload response example in §4 and the FileNode example
in §5 are usable as test oracles). The oracle is the draft, not
the code under test.

For `Sha256` parse-time validation, parametric tests cover:

- Valid: 64 lowercase hex chars (positive)
- Invalid: wrong length (63 / 65 chars), uppercase chars,
  non-hex chars, empty string

## Type-design constraints

### Extras-preservation policy (JMAP-lbdy)

The `CidCapability` struct, once it ships in a follow-up bead, will
carry the workspace's standard `extra` field per the
extras-preservation policy:

```rust
#[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
pub extra: serde_json::Map<String, serde_json::Value>,
```

`Sha256` is a newtype wrapping a single `String` value and is
**out of scope** for the extras policy per workspace AGENTS.md
(see "extras-preservation policy" → "Out of scope" → "Newtypes
wrapping a single value").

## History

- bd:JMAP-v9py is the parent epic ("Compliance sweep …
  implement draft-atwood-jmap-cid-00").
- bd:JMAP-v9py.11 (this bead) creates the crate scaffolding.
- bd:JMAP-v9py.12 / .13 / .14 (follow-ups) define the `Sha256`
  typed shape, wire it into the Blob upload response surface in
  `jmap-base-client`, and add the `supports_cid()` Session
  advertisement detection.
