# jmap-cid

JMAP Blob Content Identifiers ([draft-atwood-jmap-cid-00]) in one
dependency: re-exports [`jmap-cid-types`]. The CID family has no client or
server crate, so there are no features.

## What it is

A facade with no code of its own:

| Feature | Re-export |
|---|---|
| (none) | `jmap_cid::types` |

`types` holds the capability (`JMAP_CID_URI`, `CidCapability`) and the
`Sha256` digest type, with its CIDv1 and padded-base64 conversions.

## What it's for

CID adds a SHA-256 content identifier to blob upload responses and, with
FileNode, to FileNode objects, so a client can check content integrity.
Applications that want the CID types under the family's
umbrella name depend on this crate; workspace crates depend on
`jmap-cid-types` directly.

## How to use

```toml
[dependencies]
jmap-cid = "0.5"
```

```rust
use jmap_cid::types::{JMAP_CID_URI, Sha256};

assert_eq!(JMAP_CID_URI, "urn:ietf:params:jmap:cid");
let digest: Sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    .parse()
    .unwrap();
assert_eq!(digest.to_cidv1(), "bafkreihdwdcefgh4dqkjv67uzcmw7ojee6xedzdetojuzjevtenxquvyku");
```

## How it works

`types` is a `pub use` of `jmap-cid-types`, so `jmap_cid::types::X` is the
same item as `jmap_cid_types::X`. The component crate versions in lockstep
with this one on the shared `jmap-*` line.

## Gotchas

- Server-side `sha256` computation is the consumer's backend's job; no
  `-server` crate exists to re-export.
- Depending on this crate and on `jmap-cid-types` at a different version
  gives two copies of each type, which do not unify.
- Version 0.5.0 replaces the 0.0.1 name-claim placeholder.

## References

- [draft-atwood-jmap-cid-00] — JMAP Blob Content Identifiers
- [RFC 8620] — JMAP Core (blob upload, §6.1)

[draft-atwood-jmap-cid-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-cid-types`]: https://crates.io/crates/jmap-cid-types
