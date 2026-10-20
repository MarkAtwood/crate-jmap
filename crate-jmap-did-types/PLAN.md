# jmap-did-types — Plan

Typed JMAP surface of draft-atwood-jmap-did-00 (epic bd:JMAP-6itf).
Types and pure helpers only: no network I/O.

## Scope

| Bead | Content | State |
|---|---|---|
| JMAP-6itf.1 | Crate scaffold, `JMAP_DID_URI`, `DidCapability`, `DidAccountCapability`, `KeyCustody` | landed |
| JMAP-6itf.2 | DID URI newtype (DID Core §3.1), did:web domain extractor | landed |
| JMAP-6itf.5 | Pure did:key, did:web and PLC op-log resolvers | landed |

## Decisions

- `supportedMethods` must not be empty ("Servers MUST support at least
  one method").
- `String|null` fields are `Option` and always serialize, as `null`.
- `keyCustody` is a result enum with an `Other(String)` catch-all; `null`
  is `None`.
- did:key follows the did:key Method v0.9 algorithms (the v0.7 URL the
  draft cites is gone): `Multikey`, the did/v1.1 context, no X25519
  key-agreement derivation, length checks only.
- did:plc verifies the full audit log (`/log/audit`), not just the active
  `/log`, so nullification and ordering are checked. Submission limits
  (size, key counts, duplicate keys) are not re-checked: real logs
  predate some of them.
