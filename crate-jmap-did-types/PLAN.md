# jmap-did-types — Plan

Typed JMAP surface of draft-atwood-jmap-did-00 (epic bd:JMAP-6itf).
Types and pure helpers only: no network I/O.

## Scope

| Bead | Content | State |
|---|---|---|
| JMAP-6itf.1 | Crate scaffold, `JMAP_DID_URI`, `DidCapability`, `DidAccountCapability`, `KeyCustody` | landed |
| JMAP-6itf.2 | DID URI newtype (DID Core §3.1), did:web domain extractor | landed |
| JMAP-6itf.5 | Pure did:key, did:web and PLC op-log resolvers | landed |
| JMAP-6itf.9 | Pure did:webvh log and witness verifier | landed |

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
- did:webvh follows DIF did:webvh v1.0 and supports only
  `did:webvh:1.0` logs. The first invalid entry and every later one are
  invalid; earlier versions stay resolvable by `versionId`. The
  `versionTime` future tolerance is 5 minutes. Resolving the DID to its
  HTTPS URLs and fetching belong to `crate-jmap-did-resolver`
  (JMAP-6itf.10). The implicit `#files`/`#whois` services are DID URL
  dereferencing, not added to the logged documents.
