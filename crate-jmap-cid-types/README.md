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
helper (both landed in `jmap-base-client`), and by a future
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

Pulls in `serde` and `serde_json` at runtime. No async runtime,
no `jmap-types`, no `jmap-server`, no `jmap-base-client`. Parse
and validate a `sha256` digest from a wire string:

```rust
use jmap_cid_types::Sha256;

// 64 lowercase hex characters per draft-atwood-jmap-cid-00 §2.
let digest: Sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    .parse()?;
assert_eq!(digest.as_ref().len(), 64);
# Ok::<(), jmap_cid_types::Sha256DigestError>(())
```

Use `Sha256::from_raw_digest(&[u8; 32])` to build a digest from the
raw 32-byte output of a SHA-256 hash function (e.g. `sha2::Sha256`)
without going through a hex string. The capability URI
`"urn:ietf:params:jmap:cid"` is detected via the `Session.capabilities`
map in `jmap-base-client`.

## How it works

- [`Sha256`] is a `String`-backed newtype with parse-time ABNF
  validation per the draft `sha256-value` rule
  (`64( %x30-39 / %x61-66 )`): exactly 64 characters, lowercase hex
  only. Validation runs on `from_hex`, `TryFrom<&str>`, `FromStr`,
  and the `Deserialize` impl.
- `from_raw_digest(&[u8; 32])` builds a digest infallibly by
  formatting the raw 32-byte SHA-256 output as lowercase hex; no
  parse step is needed because the output is always valid by
  construction. The name emphasises that the input is the output
  of a hash function — this crate carries no hash computation.
- The error type [`Sha256DigestError`] is a single-tier enum
  (`WrongLength { got }` or `NonHexLowercase { at, byte }`) with
  `#[non_exhaustive]` at the type level and per-variant
  `#[non_exhaustive]` so variant additions and per-variant field
  additions both remain semver-additive. Diagnostics carry byte
  position so callers can point at the bad byte without re-indexing
  the input.
- `#[non_exhaustive]` on every public struct and enum, so additive
  spec evolution stays a non-breaking change.
- No async. `#[forbid(unsafe_code)]` at the crate root.
- Runtime dependencies limited to `serde` and `serde_json`
  (the latter for the `CidCapability.extra` Map<String, Value>
  surface per workspace extras-preservation policy).

## Gotchas

- Blob upload-response and Session-capability wiring live in
  `jmap-base-client`, not here.
  `jmap_base_client::blob::BlobUploadResponse.sha256:
  Option<jmap_cid_types::Sha256>` is the typed binding for the
  upload-response `sha256` field; `Session::supports_cid()` is the
  helper that checks the capability map. This crate is the
  wire-shape source of truth; the binding lives one layer up.
- CID and the RFC 9404 BLOBEXT `Blob/get` `digest:sha-256` request
  are deliberately separate mechanisms with different encodings
  (lowercase hex vs base64) and different access patterns
  (unconditional at upload vs on-demand via `Blob/get`). See the
  draft §2.3.

## References

- [draft-atwood-jmap-cid-00] — JMAP Blob Content Identifiers (normative
  for `Sha256` and the `urn:ietf:params:jmap:cid` capability)
- [RFC 8620] — JMAP Core (Blob upload §6.1 is the binding point)

[draft-atwood-jmap-cid-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
