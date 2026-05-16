# jmap-cid-types — Implementation Plan

draft-atwood-jmap-cid-00 wire-format types for the `jmap-*` crate
family. Pure types — no method handlers, no async, no network I/O.

## Crate family position

```
jmap-types
    └── jmap-cid-types  ← this crate
```

Consumers (status as of bd:JMAP-sf5h close-out):

- `jmap-base-client` (**landed** under bd:JMAP-v9py.13 / .14) —
  Blob upload response carries a typed `sha256:
  Option<jmap_cid_types::Sha256>` field
  (`crate-jmap-base-client/src/blob.rs:83`); `Session::supports_cid()`
  is the capability advertisement check
  (`crate-jmap-base-client/src/request.rs:226`).
- A future `jmap-filenode-types` revision — FileNode object gains
  a typed `sha256: Option<Sha256>` property when both CID and
  FileNode capabilities are advertised. Not yet scheduled.

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
[dependencies]
serde      = { workspace = true }
serde_json = { workspace = true }
```

The crate carries no `jmap-types` runtime dependency: `Sha256` uses
only `serde` and `std`. The reservation that originally appeared
here (per bd:JMAP-v9py.11 acceptance criteria) was dropped under
bd:JMAP-sf5h.2 after the type landed without referencing
`jmap-types`. If a future surface (e.g. an `Id`-shaped typed
blob-reference helper) needs `jmap-types`, re-add it in the same
commit that introduces the consumer code.

`serde_json` is a runtime dependency because `CidCapability.extra`
is typed as `serde_json::Map<String, serde_json::Value>` per the
workspace extras-preservation policy. It was briefly moved to
`[dev-dependencies]` under bd:JMAP-sf5h.3 (when only the `Sha256`
type was present) and moved back when `CidCapability` landed under
bd:JMAP-sf5h.6.

## Public API (current state)

After bd:JMAP-v9py.12:

- `pub struct Sha256(String);` — `#[serde(try_from = "String", into
  = "String")]` newtype with parse-time ABNF validation
  (`64( %x30-39 / %x61-66 )`). Implements `Display`, `AsRef<str>`,
  `From<Sha256> for String`, `TryFrom<String>`, `TryFrom<&str>`,
  `FromStr`. Constructors: `from_hex(&str)` (validating) and
  `from_raw_digest(&[u8; 32])` (infallible nibble-formatter; the
  name disambiguates from `sha2::Sha256::digest`-style "compute the
  hash of these bytes" — see bd:JMAP-sf5h.4).
- `pub enum Sha256DigestErrorKind { WrongLength { got }, NonHexLowercase { at } }`
  and `pub struct Sha256DigestError { kind: Sha256DigestErrorKind }`
  — both `#[non_exhaustive]`, position-tracking diagnostic for
  failed parses.

- `pub struct CidCapability` (landed under bd:JMAP-sf5h.6) — the
  value object of `urn:ietf:params:jmap:cid` (draft §3). Currently
  empty; `#[non_exhaustive]` with the workspace's standard `extra`
  field per the extras-preservation policy.

Module layout mirrors `jmap-metadata-types`: `digest.rs` for the
`Sha256` type, `capability.rs` for the capability URI constant
(`JMAP_CID_URI`) and the `CidCapability` value object, and
`lib.rs` re-exports.

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

### Sibling-of-Id newtype, fallible construction only (bd:JMAP-sf5h.7)

`Sha256` mirrors the sibling `jmap-types::Id` newtype surface
(`PartialEq<str>`, `PartialEq<&str>`, `Borrow<str>`,
`AsRef<str>`, `Display`, `FromStr`, `into_inner`) so it reads like
every other workspace wire-format newtype. The infallible
`From<String>` / `From<&str>` impls that `Id`, `UTCDate`, `Date`,
`State` carry via `impl_string_newtype!` are **deliberately
omitted** from `Sha256`.

- `Id`'s character set is open: RFC 8620 §1.2 SAFE-CHAR
  (visible ASCII minus DQUOTE), 1..=255 bytes, server-assigned,
  opaque to the client. Treating an arbitrary `String` as an `Id`
  cannot produce a wire-protocol mismatch because the protocol
  itself doesn't constrain content beyond the byte set.
- `Sha256`'s character set is closed: draft-atwood-jmap-cid-00 §2
  ABNF `64( %x30-39 / %x61-66 )` — exactly 64 bytes, lowercase
  hex only. Both sides validate. An infallible `From<String>`
  would let arbitrary garbage round-trip through serialize and
  emerge as a malformed wire field.

Construction is strictly fallible: `from_hex(&str)`,
`TryFrom<&str>`, `TryFrom<String>`, `FromStr`, and the
`Deserialize` adapter all share the same `validate(&str)` ABNF
check. The decision is normative — a future contributor proposing
`From<String> for Sha256` for canonical-template consistency
should be referred to this section.

### NIST FIPS 180-4 oracle for from_raw_digest tests (bd:JMAP-sf5h.8)

`from_raw_digest_formats_canonical_lowercase_hex` uses the SHA-256
of the empty string (`e3b0c4...b855`) hand-copied from NIST
FIPS 180-4 as the test oracle. The vector is **not** computed at
test time via `sha2::Sha256::digest()`. Doing so would close the
oracle loop: any nibble-ordering or character-table bug in
`from_raw_digest` would emit the same wrong digest the test
expects, and the test would still pass. The independent oracle
catches that class of bug. The decision is workspace-policy
("Test oracles must be independent of the code under test" —
workspace AGENTS.md "Test Integrity").

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
- bd:JMAP-v9py.11 created the crate scaffolding.
- bd:JMAP-v9py.12 added the `Sha256` typed shape with parse-time
  ABNF validation.
- bd:JMAP-v9py.13 (closed 2026-05-13) wired the `Sha256` field
  into the Blob upload response surface in `jmap-base-client`
  (`crate-jmap-base-client/src/blob.rs:83`).
- bd:JMAP-v9py.14 (closed 2026-05-13) added the `supports_cid()`
  Session advertisement detection in `jmap-base-client`
  (`crate-jmap-base-client/src/request.rs:226`).
- bd:JMAP-sf5h is the post-landing review epic. Findings .11
  (JMAP_CID_URI constant), .10 (per-variant non_exhaustive on
  Sha256DigestErrorKind), .4 (from_bytes → from_raw_digest rename),
  .3 (serde_json → dev-dependencies), and .1 (this docs sweep)
  closed in the same pass; remaining children track API-contract
  forward-compat and idiom-pass findings.
