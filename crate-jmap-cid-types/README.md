# jmap-cid-types

draft-atwood-jmap-cid-00 wire-format types for the `jmap-*` crate family.

The `urn:ietf:params:jmap:cid` JMAP capability plus the `sha256` typed
string shape used in blob upload responses and FileNode objects.

## What it is

When a server advertises `urn:ietf:params:jmap:cid`, blob upload
responses (RFC 8620 §6.1) carry a `sha256` field with the SHA-256
digest of the uploaded content, encoded as a lowercase hexadecimal
string of exactly 64 characters. When the JMAP FileNode extension is
also advertised, FileNode objects gain the same `sha256` property.

This crate carries the typed `sha256` wire shape ([`Sha256`]) plus
the matching parse-error type ([`Sha256DigestError`]). Wiring the
shape into the Blob upload response surface and Session capability
detection happens in `jmap-base-client`.

## Why a separate crate

CID is not tied to any single consumer extension. The `sha256` field
is referenced by `draft-atwood-jmap-chat-00` (which defers to the CID
document as the normative definition), and the spec is structured to
fold cleanly into a future RFC 8620 bis or
`draft-ietf-jmap-filenode`. Standing CID up as its own crate keeps
the dep graph honest and avoids forcing a `jmap-chat-*` or
`jmap-filenode-*` dependency on consumers that only want content
identifiers.

## What it's for

The typed wire shape here is consumed by `jmap-base-client` for the
`BlobUploadResponse.sha256` field and the `Session::supports_cid()`
helper (per the per-bead plan in `PLAN.md`), and by a future
`jmap-filenode-types` revision for the FileNode `sha256` property.
The CID document — capability URI `urn:ietf:params:jmap:cid`, draft
owned by Mark Atwood — is deliberately independent of any single
consumer extension because the same content-hash field feeds Blob
upload responses, FileNode integrity, and the JMAP Chat draft's
attachment references.

## How to use

```toml
[dependencies]
jmap-cid-types = "0.1"
```

Transitively pulls in `jmap-types`, `serde`, `serde_json`. No async
runtime, no `jmap-server`, no `jmap-base-client`. Parse and validate
a `sha256` digest from a wire string:

```rust
use jmap_cid_types::Sha256;

// 64 lowercase hex characters per draft-atwood-jmap-cid-00 §2.
let digest: Sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    .parse()?;
assert_eq!(digest.as_ref().len(), 64);
# Ok::<(), jmap_cid_types::Sha256DigestError>(())
```

Use `Sha256::from_bytes(&[u8; 32])` to build a digest from raw bytes
without going through a hex string. The capability URI
`"urn:ietf:params:jmap:cid"` is detected via the `Session.capabilities`
map in `jmap-base-client`.

## How it works

- [`Sha256`] is a `String`-backed newtype with parse-time ABNF
  validation per the draft `sha256-value` rule
  (`64( %x30-39 / %x61-66 )`): exactly 64 characters, lowercase hex
  only. Validation runs on `from_hex`, `TryFrom<&str>`, `FromStr`,
  and the `Deserialize` impl.
- `from_bytes(&[u8; 32])` builds a digest infallibly by formatting
  the bytes as lowercase hex; no parse step is needed because the
  output is always valid by construction.
- The error type [`Sha256DigestError`] carries a position-tracking
  [`Sha256DigestErrorKind`] (`WrongLength { got }` or
  `NonHexLowercase { at }`) so diagnostics can point at the bad
  byte.
- `#[non_exhaustive]` on every public struct and enum, so additive
  spec evolution stays a non-breaking change.
- No async. `#[forbid(unsafe_code)]` at the crate root.
- Dependencies limited to `jmap-types`, `serde`, `serde_json`.

## Gotchas

- This crate is a skeleton at the current draft revision
  (draft-atwood-jmap-cid-00; bd:JMAP-v9py.11). Today it exports only
  the [`Sha256`] type and its error machinery. The `CidCapability`
  marker struct and the wiring into Blob upload responses /
  `Session::supports_cid()` land in follow-up beads (bd:JMAP-v9py.13
  and bd:JMAP-v9py.14) — see `PLAN.md`.
- The `sha256` typed wire shape is NOT yet plumbed through every
  blob-response shape across the workspace. The integration point
  is `jmap-base-client`; check there for the actual round-trip
  binding before relying on a typed `sha256` field in a JMAP
  response.
- CID and the RFC 9404 BLOBEXT `Blob/get` `digest:sha-256` request
  are deliberately separate mechanisms with different encodings
  (lowercase hex vs base64) and different access patterns
  (unconditional at upload vs on-demand via `Blob/get`). See the
  draft §2.3.

## References

- [draft-atwood-jmap-cid-00] — JMAP Blob Content Identifiers (normative
  for `Sha256` and the `urn:ietf:params:jmap:cid` capability)
- [RFC 8620] — JMAP Core (Blob upload §6.1 is the binding point)

[draft-atwood-jmap-cid-00]: https://datatracker.ietf.org/doc/draft-atwood-jmap-cid/
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
