# jmap-chat-mimi-types — Plan

Typed JMAP surface of draft-atwood-jmap-chat-mimi-00 (epic bd:JMAP-fvnd).
Types only: no server logic, no I/O, no MLS or TLS-codec dependency.

## Scope

| Bead | Content | State |
|---|---|---|
| JMAP-fvnd.1 | Crate scaffold, `JMAP_CHAT_MIMI_URI`, `MimiCapability`, body-type constants, `Base64Url` | landed |
| JMAP-fvnd.2 | KeyPackage object, KeyPackage/get and /set | landed |
| JMAP-fvnd.3 | KeyMaterial/fetch request and response | landed |
| JMAP-fvnd.4 | Room/create, Room/update, Room/send, Room/unbind, `chat_room_id` | landed |
| JMAP-fvnd.5 | GroupInfo/fetch request and response | landed |

## Decisions

- Byte-string properties are `Base64Url`, kept verbatim and never
  re-encoded (draft §Conventions). Validation rejects non-zero pad bits, so
  string equality is octet equality.
- Cipher suites are `u16`, the width of RFC 9420 `uint16 CipherSuite`.
- The session-level capability value `{}` is not modelled, as in
  `jmap-chat-types` for its other empty session values.
- `/get` and `/set` responses reuse `jmap_types::GetResponse` and
  `SetResponse`. `SetResponse<KeyPackageCreated>` carries the `created`
  entries, which hold only the properties the client did not send
  (RFC 8620 §5.3).
- `userStatus` and `clientStatus` are result enums with an `Other(String)`
  catch-all, named as in draft-ietf-mimi-protocol-06 §5.2. The
  `keyPackage`-iff-`success` rule is documented, not enforced: a status-tagged
  enum would drop unknown codes.
- No dependency on `jmap-chat-types`: string constants carry the body
  types, and consumers map them to `BodyType::Other`. `chat_room_id` takes
  `&Chat.extra` rather than `&Chat` for the same reason; `jmap-chat-types`
  is a dev-dependency only, to test against a real `Chat`.
- `Room/update` and `Room/send` fields the draft marks "When
  `responseCode` is ..." are `Option`, documented not enforced, as for
  `keyPackage`.
