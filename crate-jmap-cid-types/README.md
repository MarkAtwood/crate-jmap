# jmap-cid-types

draft-atwood-jmap-cid-00 wire-format types for the `jmap-*` crate family.

The `urn:ietf:params:jmap:cid` JMAP capability plus the `sha256` typed
string shape used in blob upload responses and FileNode objects.

## What

When a server advertises `urn:ietf:params:jmap:cid`, blob upload
responses (RFC 8620 §6.1) carry a `sha256` field with the SHA-256
digest of the uploaded content, encoded as a lowercase hexadecimal
string of exactly 64 characters. When the JMAP FileNode extension is
also advertised, FileNode objects gain the same `sha256` property.

This crate carries the capability marker plus the typed `sha256`
string shape. Wiring it into the Blob upload response surface and
Session capability detection happens in `jmap-base-client`.

## Why a separate crate

CID is not tied to any single consumer extension. The `sha256` field
is referenced by `draft-atwood-jmap-chat-00` (which defers to the CID
document as the normative definition), and the spec is structured to
fold cleanly into a future RFC 8620 bis or
`draft-ietf-jmap-filenode`. Standing CID up as its own crate keeps
the dep graph honest and avoids forcing a `jmap-chat-*` or
`jmap-filenode-*` dependency on consumers that only want content
identifiers.

## Dependencies

```toml
jmap-cid-types = "0.1"
```

Transitively pulls in `jmap-types`, `serde`, `serde_json`. No async
runtime, no `jmap-server`, no `jmap-base-client`.

## Status

Skeleton (bd:JMAP-v9py.11). The `Sha256` typed shape, Blob upload
wiring, and capability advertisement detection land in follow-up
beads — see `PLAN.md`.

## License

MIT OR Apache-2.0
