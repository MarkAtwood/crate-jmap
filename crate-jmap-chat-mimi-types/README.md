# jmap-chat-mimi-types

Wire-format types for draft-atwood-jmap-chat-mimi-00, the
`urn:ietf:params:jmap:chat:mimi` capability with which an MLS client uses a
JMAP Chat server as its MIMI provider. Types only: no I/O, no MLS parsing.

## What it is

| Item | Spec |
|---|---|
| `JMAP_CHAT_MIMI_URI` | §The urn:ietf:params:jmap:chat:mimi Capability |
| `MimiCapability` | §Account-Level Capability Object |
| `MIMI_FANOUT_BODY_TYPE` (`application/mimi-fanout`) | §application/mimi-fanout |
| `MLS_MESSAGE_BODY_TYPE` (`message/mls`) | §message/mls, RFC 9420 §17.10 |
| `KeyPackage`, `KeyPackageCreate`, `KeyPackageCreated`, `KeyPackageProperty` | §KeyPackages |
| `KeyMaterialFetchRequest`, `KeyMaterialFetchResponse`, `ClientKeyMaterial`, `KeyMaterialUserCode`, `KeyMaterialClientCode` | §KeyMaterial/fetch, draft-ietf-mimi-protocol-06 §5.2 |
| `chat_room_id` | §Chat.roomId |
| `RoomCreateRequest`, `RoomCreateResponse` | §Room/create |
| `RoomUpdateRequest`, `RoomUpdateResponse`, `UpdateResponseCode` | §Room/update, draft-ietf-mimi-protocol-06 §5.3 |
| `RoomSendRequest`, `RoomSendResponse`, `SubmitResponseCode` | §Room/send, draft-ietf-mimi-protocol-06 §5.4 |
| `RoomUnbindRequest`, `RoomUnbindResponse` | §Room/unbind |
| `Base64Url`, `Base64UrlError` | §Conventions and Definitions, RFC 4648 §5 |

The session-level capability value is `{}` and has no type.

## What it's for

It gives the MIMI provider server (mimid and the planned
`jmap-chat-mimi-server`) and its clients one definition of the extension's
wire types. It sits beside `jmap-chat-types` and does not depend on it;
it reuses `jmap-types` for `Id` and the RFC 8620 `/get` and `/set`
response shapes.

## How to use

```rust
use jmap_chat_mimi_types::MimiCapability;

let cap: MimiCapability = serde_json::from_str(r#"{
    "userUri": "mimi://b.example/u/bob",
    "roomIdPrefix": "mimi://b.example/r/",
    "hubSender": "AAEC",
    "cipherSuites": [1],
    "maxKeyPackages": 100
}"#)?;
assert_eq!(cap.max_key_packages, 100);
# Ok::<(), serde_json::Error>(())
```

## How it works

- Every byte-string property is a `Base64Url`. It checks the string and
  keeps it verbatim, because the draft forbids re-encoding the octets.
  It rejects padding, the standard `+`/`/` alphabet, impossible lengths and
  non-zero pad bits, so equal strings mean equal octets.
- `#[non_exhaustive]` on every public struct and enum, plus an `extra` map
  for vendor fields, per workspace policy.
- No async. `#[forbid(unsafe_code)]` at the crate root.

## Gotchas

- `cipherSuites` must be non-empty and every entry must fit in `u16`
  (RFC 9420 `uint16 CipherSuite`). Otherwise deserialize fails.
- `Base64Url` does not decode. Decode with your own base64 and TLS-codec
  stack.
- `Base64Url` allocates the whole input before it validates it. Bound body
  sizes at the transport layer.

- `KeyPackage/set` responses are `jmap_types::SetResponse<KeyPackageCreated>`,
  not `SetResponse<KeyPackage>`: a `created` entry holds only `id`,
  `keyPackageRef` and, if the create omitted it, `lastResort`.
- `ClientKeyMaterial` does not check that `keyPackage` is present exactly
  when `clientStatus` is `success`. Check it before use.

## References

- [draft-atwood-jmap-chat-mimi-00] — JMAP Chat MIMI provider
- [RFC 9420] — The Messaging Layer Security (MLS) Protocol
- [RFC 4648] — Base16, Base32, and Base64 Data Encodings (§5)
- [RFC 8620] — JMAP Core

[draft-atwood-jmap-chat-mimi-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 9420]: https://www.rfc-editor.org/rfc/rfc9420
[RFC 4648]: https://www.rfc-editor.org/rfc/rfc4648
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
