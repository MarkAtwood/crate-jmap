# jmap-chat-mimi-server

Server rules for draft-atwood-jmap-chat-mimi-00, the
`urn:ietf:params:jmap:chat:mimi` capability with which an MLS client uses a
JMAP Chat server as its MIMI provider. Backend-agnostic handlers for a
`jmap-server` `Dispatcher`; you supply storage through the `MimiBackend`
trait.

## What it is

| Item | Spec |
|---|---|
| `handle_key_package_get` | §KeyPackage/get (RFC 8620 §5.1) |
| `handle_key_package_set` | §KeyPackage/set: create, `forbidden` update, destroy |
| `check_key_package_create` | §KeyPackage/set create checks, §KeyPackage Object Fields `lastResort` |
| `signature_key_bound_elsewhere`, `key_package_ref_in_use`, `key_package_over_quota` | §KeyPackage/set account-wide rules |
| `MimiBackend`, `NewKeyPackage` | crypto provider, capability object, atomic KeyPackage create and destroy |
| `MimiBackend::claim_key_packages`, `KeyMaterialRequirements`, `KeyPackageFit` | §Handing Out KeyPackages: atomic claim, `clientUri`, KeyPackageRef retention |
| `handle_room_create`, `check_room_create` | §Room/create: the seven checks and the bound Chat |
| `MimiBackend::create_room`, `NewRoom`, `room_signature_key_not_bound`, `room_id_in_use`, `group_id_in_use` | §Room/create checks over server state, atomic with the insert |
| `may_update_room_chat`, `may_create_room_message`, `may_update_room_message` | §Chat.roomId and §message/mls restrictions, for `jmap-chat-server` `ChatBackend` hooks |
| `parse_fanout_messages`, `Fanout`, `FanoutRecipients`, `FanoutParseError` | §application/mimi-fanout: split a `/notify` body, `senderMsgId`, `sentAt`, Welcome KeyPackageRefs |
| `MimiBackend::store_fanout`, `MimiBackend::unbind_room` | §application/mimi-fanout storage, Welcome routing, duplicate rules, hub ChatContact; §Chat.roomId bind lifecycle |
| `handle_key_material_fetch` | §KeyMaterial/fetch: checks, local answer as hub, or the request to the hub |
| `handle_room_update`, `handle_room_send` | §Room/update, §Room/send: submit to the hub, store the accepted FanoutMessage |
| `handle_room_unbind` | §Room/unbind |
| `handle_group_info_fetch` | §GroupInfo/fetch: forward, return the GroupInfoResponse unchanged |
| `MimiBackend::room_bound`, `MimiBackend::local_key_material_account` | "no Chat in the account is bound to `roomId`"; local target in a hosted room |
| `MimiBackend::mimi_request`, `MimiEndpoint`, `HubReply`, `HubUnavailable` | MIMI requests to the hub or target provider; `serverUnavailable` |
| `memory::MemoryBackend` (`memory` feature) | in-memory reference backend |
| `register_mimi_handlers` | wires every method above |

## What it's for

A MIMI provider (mimid, or any JMAP Chat server that routes MLS messages
for its clients) wires these handlers next to `jmap-chat-server`. The crate
parses and verifies KeyPackages with `openmls`; it never holds MLS secrets
and never decrypts. It does not implement the hub side of the MIMI
protocol: `MimiBackend::mimi_request` sends each request to the room's
hub, or processes it as hub when this server is the hub.

## How to use

```rust,ignore
use std::sync::Arc;
use jmap_chat_mimi_server::{memory::MemoryBackend, register_mimi_handlers};
use jmap_chat_mimi_server::openmls_rust_crypto::RustCrypto;
use jmap_server::Dispatcher;

let backend = Arc::new(MemoryBackend::new(RustCrypto::default()));
let mut dispatcher: Dispatcher<()> = Dispatcher::new();
register_mimi_handlers(&mut dispatcher, backend);
```

Advertise `urn:ietf:params:jmap:chat:mimi` in your Session with the same
account-level object that `MimiBackend::mimi_capability` returns.

## How it works

A create is decoded (`keyPackage` required, `lastResort` optional, nothing
else), then `check_key_package_create` decodes the base64url octets, parses
them as an RFC 9420 KeyPackage, verifies it with openmls and checks the
cipher suite against `cipherSuites`, the Basic credential and its identity
against `userUri`. It derives `lastResort` from the `app_data_dictionary`
extension's `last_resort_key_package` component and computes the
KeyPackageRef over the exact octets the client sent. Every failure is
`invalidProperties` naming `keyPackage`, or `lastResort` for a differing
claim.

`MimiBackend::create_key_package` then applies the account-wide rules in
one atomic step: the signature-key binding, KeyPackageRef reuse,
replacement of the client's previous last-resort KeyPackage, and
`maxKeyPackages`. These are uniqueness and count constraints over stored
state, so they belong in the backend's transaction, not in a handler-side
read that a concurrent create could race.

`MimiBackend::claim_key_packages` answers a key material request for the
owner the same way: in one atomic step it picks, for each client
(signature key bound to the account), a KeyPackage that
`KeyMaterialRequirements::fit` calls compatible, destroys it unless it is
the client's last-resort KeyPackage, and records its KeyPackageRef so a
later create of the same octets is refused. A client with only
incompatible KeyPackages is `nothingCompatible`; one with none left, or
only expired ones, is `keyMaterialExhausted`.

`Room/create` splits the same way. `check_room_create` parses the
GroupInfo and ratchet tree, has openmls `PublicGroup::from_external` check
the GroupInfo signature and the tree hash, and checks the one leaf, the
`external_senders` extension against `hubSender`, and the
`participant_list`, `room_metadata`, `franking_agent` and
`base_room_policy` components of the `app_data_dictionary`.
`MimiBackend::create_room` then checks that the leaf's signature key is
bound to the account and that the room ID and group ID are unused, stores
the room and creates the bound Chat. Every refusal is the method-level
`invalidArguments`.

`KeyMaterial/fetch`, `Room/update`, `Room/send` and `GroupInfo/fetch`
decode the client's MIMI request (draft-ietf-mimi-protocol-06 §5.2-§5.6)
and require `MimiBackend::room_bound`; anything else is
`invalidArguments`. `Room/send` wraps the message in a
SubmitMessageRequest with `sendingUri` set to `userUri`; the others send
the request unchanged. `MimiBackend::mimi_request` returns the response,
or `HubUnavailable`, which is `serverUnavailable`. The handler decodes the
response into the method's arguments, and on success `Room/update` and
`Room/send` build the FanoutMessage the hub will not send back and store it
with `MimiBackend::store_fanout`, attributed to `HubReply::provider_uri`.
In a room this server hosts, `KeyMaterial/fetch` for a local target
(`MimiBackend::local_key_material_account`) is answered with
`claim_key_packages` instead: `success` when every client got a
KeyPackage, `partialSuccess` when some did, otherwise
`noCompatibleMaterial` with each client's status.

A Chat with a `roomId` changes only through MLS, so its `ChatBackend`
returns `may_update_room_chat`, `may_create_room_message` and
`may_update_room_message` from the `may_update_chat`,
`may_create_message` and `may_update_message` hooks, passing
`&chat.extra`.

## Gotchas

- MSRV 1.91, set by `openmls` 0.9.2.
- `openmls` and `openmls_rust_crypto` are public dependencies, re-exported
  as `jmap_chat_mimi_server::openmls` and
  `jmap_chat_mimi_server::openmls_rust_crypto`; use those paths rather
  than pinning your own copies. A new `openmls` minor version means a
  major release of this crate.
- Enable `extensions-draft` on every other openmls-family crate in your
  build, for example a storage crate. This crate turns it on in `openmls`, which turns
  it on in `openmls_traits`, and a storage crate built without it then
  fails to compile.
- openmls 0.9.2 rejects a KeyPackage that carries a GREASE extension
  (RFC 9420 §13.5) with `UnsupportedExtension`, so such a KeyPackage is
  refused with `invalidProperties` naming `keyPackage` (bd:JMAP-6k5d.9).
- openmls checks the leaf node lifetime against the system clock and also
  refuses a lifetime that has not started yet.
- `lastResort` comes from the `app_data_dictionary` component of
  draft-ietf-mls-extensions-10 §6.4 only. openmls's
  `KeyPackage::last_resort`, which reads the older `last_resort` extension
  type, is not consulted.
- A KeyPackage refused by the per-KeyPackage checks never reaches the
  backend, so it binds no signature key.
- `KeyMaterialRequirements::fit` verifies the stored KeyPackage again, so
  a claim costs one signature verification per stored KeyPackage of the
  client examined.
- The `base_room_policy` component ID is 0x0027, the value
  draft-ietf-mimi-room-policy-04 §10.1.4 suggests; IANA has not assigned
  one.
- The KeyMaterialRequest and GroupInfoRequest signatures are not
  verified; the draft does not ask the provider to, and the hub does.
- The UpdateRequest handshake must be a PublicMessage: openmls 0.9.2 does
  not decode a SemiPrivateMessage.
- A hub response that does not parse is `serverFail`, not
  `invalidArguments`.
- The `memory` backend keeps handed-out KeyPackageRefs forever; the draft
  lets a backend drop one once a Welcome naming it is delivered or its
  lifetime passes.

## References

- [draft-atwood-jmap-chat-mimi-00] — JMAP Chat MIMI provider
- [RFC 9420] — The Messaging Layer Security (MLS) Protocol
- [draft-ietf-mls-extensions-10] — The Messaging Layer Security (MLS) Extensions
- [draft-ietf-mimi-protocol-06] — MIMI protocol (components, §5.4.1, §7.5, §7.6)
- [draft-ietf-mimi-room-policy-04] — MIMI room policy (`BaseRoomPolicy`, §5)
- [RFC 8620] — JMAP Core

[draft-atwood-jmap-chat-mimi-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 9420]: https://www.rfc-editor.org/rfc/rfc9420
[draft-ietf-mls-extensions-10]: https://datatracker.ietf.org/doc/draft-ietf-mls-extensions/
[draft-ietf-mimi-protocol-06]: https://datatracker.ietf.org/doc/draft-ietf-mimi-protocol/
[draft-ietf-mimi-room-policy-04]: https://datatracker.ietf.org/doc/draft-ietf-mimi-room-policy/
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
