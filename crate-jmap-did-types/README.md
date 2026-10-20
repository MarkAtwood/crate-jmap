# jmap-did-types

draft-atwood-jmap-did-00 wire-format types for the `jmap-*` crate family.

The `urn:ietf:params:jmap:did` JMAP capability: the session-level and
account-level capability objects that bind each account to a DID
(W3C Decentralized Identifier).

## What it is

When a server advertises `urn:ietf:params:jmap:did`, the session-level
capability lists the DID methods it can resolve and verify, and each
account's capability carries the DID bound to that account, an optional
DID document URI, the DID before the last migration, and who holds the
DID's authentication keys.

## How to use

```toml
[dependencies]
jmap-did-types = "0.3"
```

```rust
use jmap_did_types::{DidAccountCapability, KeyCustody};

let cap: DidAccountCapability = serde_json::from_str(r#"{
    "did": "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
    "didDocumentUri": null,
    "previousDid": null,
    "keyCustody": "user"
}"#)?;
assert_eq!(cap.key_custody, Some(KeyCustody::User));
# Ok::<(), serde_json::Error>(())
```

## How it works

- `DidCapability` rejects an empty `supportedMethods` ("Servers MUST
  support at least one method").
- The `String|null` fields are `Option`s that always serialize, as
  `null` when `None`, matching the draft examples.
- `KeyCustody` is `Server`, `User` or `Other(String)`; JSON `null`
  ("does not disclose custody") is `None`. Never treat `None` or
  `Other` as `User`.
- `Did` parses a string against the DID Core §3.1 ABNF (generic
  syntax only, lowercase `did:` prefix, no normalization).
  `Did::web_domain` returns the host of a `did:web` DID without its
  `%3A<port>` suffix, the part clients "SHOULD highlight" against
  look-alike domains.
- `DidKey::from_did` decodes a `did:key` (base58-btc multicodec) to its
  key type and raw public key, and `DidKey::document` derives the DID
  document (`Multikey`, no key-agreement derivation).
- `Did::web_document_url` maps a `did:web` DID to the HTTPS URL of its
  `did.json`; fetching it is up to you.
- `verify_plc_audit_log` takes the bytes of a PLC directory
  `/<did>/log/audit` response and replays it: the genesis operation must
  hash to the DID, every operation must be signed (low-S ECDSA, P-256 or
  secp256k1) by a rotation key of its `prev`, recoveries must fall in
  the 72-hour window and be signed by a higher-authority key, and every
  `cid`, `nullified` flag and `createdAt` must agree. It returns
  `PlcStatus::Active(PlcState)` or `PlcStatus::Deactivated`.
- `#[non_exhaustive]` on every public type, an `extra` map for unknown
  fields, no async, `#[forbid(unsafe_code)]`.

## Gotchas

- `DidAccountCapability.did` and `previousDid` are still plain
  strings; parse them with `Did::parse` to validate.

## References

- [draft-atwood-jmap-did-00] — JMAP Decentralized Identifiers
- [DID Core] — W3C Decentralized Identifiers v1.0
- [did:key] — The did:key Method v0.9
- [did:web] — did:web Method Specification
- [did:plc] — did:plc Method Specification v0.3.0
- [RFC 8620] — JMAP Core (Session resource capabilities)

[draft-atwood-jmap-did-00]: https://github.com/MarkAtwood/jmap-chat-spec
[DID Core]: https://www.w3.org/TR/did-core/
[did:key]: https://w3c-ccg.github.io/did-key-spec/
[did:web]: https://w3c-ccg.github.io/did-method-web/
[did:plc]: https://web.plc.directory/spec/v0.1/did-plc
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
