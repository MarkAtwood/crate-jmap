# jmap-did

JMAP Decentralized Identifiers ([draft-atwood-jmap-did-00]) in one
dependency: re-exports [`jmap-did-types`]. The DID family has no client or
server crate, so there are no features.

## What it is

A facade with no code of its own:

| Feature | Re-export |
|---|---|
| (none) | `jmap_did::types` |

`types` holds the capability objects (`DidCapability`,
`DidAccountCapability`, `JMAP_DID_URI`), the `Did` syntax type, `did:key`
decoding (`DidKey`), and the `did:plc` and `did:webvh` log verifiers.

## What it's for

DIDs bind a JMAP account to a decentralized identifier, which the Chat
federation and peer-auth crates use to authenticate peers. Applications
that want the DID types under the family's umbrella name depend on this
crate; workspace crates depend on `jmap-did-types` directly.

## How to use

```toml
[dependencies]
jmap-did = "0.5"
```

```rust
use jmap_did::types::{DidAccountCapability, JMAP_DID_URI, KeyCustody};

assert_eq!(JMAP_DID_URI, "urn:ietf:params:jmap:did");
let cap: DidAccountCapability = serde_json::from_str(r#"{
    "did": "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
    "didDocumentUri": null, "previousDid": null, "keyCustody": "user"}"#)
    .unwrap();
assert_eq!(cap.key_custody, Some(KeyCustody::User));
```

## How it works

`types` is a `pub use` of `jmap-did-types`, so `jmap_did::types::X` is the
same item as `jmap_did_types::X`. The component crate versions in lockstep
with this one on the shared `jmap-*` line.

## Gotchas

- DID resolution (`did:web`, `did:plc`, `did:webvh` fetches and the
  refresh-on-failure cache) is in the support crate [`jmap-did-resolver`],
  which does network I/O and is not re-exported here. Depend on it
  directly.
- Depending on this crate and on `jmap-did-types` at a different version
  gives two copies of each type, which do not unify.
- Version 0.5.0 replaces the 0.0.1 name-claim placeholder.

## References

- [draft-atwood-jmap-did-00] — JMAP Decentralized Identifiers
- [RFC 8620] — JMAP Core

[draft-atwood-jmap-did-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-did-types`]: https://crates.io/crates/jmap-did-types
[`jmap-did-resolver`]: https://crates.io/crates/jmap-did-resolver
