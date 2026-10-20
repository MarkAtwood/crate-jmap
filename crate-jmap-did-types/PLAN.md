# jmap-did-types — Plan

Typed JMAP surface of draft-atwood-jmap-did-00 (epic bd:JMAP-6itf).
Types and pure helpers only: no network I/O.

## Scope

| Bead | Content | State |
|---|---|---|
| JMAP-6itf.1 | Crate scaffold, `JMAP_DID_URI`, `DidCapability`, `DidAccountCapability`, `KeyCustody` | landed |
| JMAP-6itf.2 | DID URI newtype (DID Core §3.1), did:web domain extractor | landed |
| JMAP-6itf.5 | Pure did:key, did:web and PLC op-log resolvers | open |

## Decisions

- `supportedMethods` must not be empty ("Servers MUST support at least
  one method").
- `String|null` fields are `Option` and always serialize, as `null`.
- `keyCustody` is a result enum with an `Other(String)` catch-all; `null`
  is `None`.
