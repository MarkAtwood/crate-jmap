# jmap-chat-mimi-server — Plan

Server rules of draft-atwood-jmap-chat-mimi-00 (epic bd:JMAP-6k5d), on top
of `jmap-chat-mimi-types`.

## Scope

| Bead | Content | State |
|---|---|---|
| JMAP-6k5d.2 | Crate scaffold, `MimiBackend`, KeyPackage/get and /set, `memory` backend | landed |
| JMAP-6k5d.3 | KeyPackage hand-out: atomic claim, `clientUri`, ref retention | landed |
| JMAP-6k5d.4 | Room/create checks and bound Chat restrictions | landed |
| JMAP-6k5d.5 | Room/update, send, unbind, GroupInfo and KeyMaterial fetch | open |
| JMAP-6k5d.6 | Fanout storage | open |
| JMAP-6k5d.9 | Upstream openmls GREASE KeyPackage fix, then bump the pin | open |

## Decisions

- MLS parsing and verification through crates.io openmls only
  (bd:JMAP-6k5d.1); the workspace MSRV is 1.91 for it (bd:JMAP-6k5d.2).
- `lastResort` is the draft's `app_data_dictionary` /
  `last_resort_key_package` component, not openmls's older `last_resort`
  extension.
- Per-KeyPackage checks are a pure function (`check_key_package_create`);
  binding, KeyPackageRef reuse, last-resort replacement and
  `maxKeyPackages` are backend rules applied atomically with the insert.
- The draft defines no `KeyPackage/changes` or `/query`; only `/get` and
  `/set` are registered.
- KeyPackage hand-out is the backend's atomic `claim_key_packages`; the
  `clientUri` scheme is the backend's choice. Releasing a handed-out
  KeyPackageRef when its Welcome is delivered belongs with fanout
  (JMAP-6k5d.6).
- `Room/create` checks that need no server state are a pure function
  (`check_room_create`); the signature-key binding and room ID and group
  ID uniqueness are `MimiBackend::create_room`'s, atomic with storing the
  room and creating the bound Chat. GroupInfo signature and tree hash go
  through openmls `PublicGroup::from_external` over a throwaway
  `openmls_memory_storage`.
- MIMI components are decoded by local tls_codec structs written from
  draft-ietf-mimi-protocol-06 and draft-ietf-mimi-room-policy-04; mimid's
  decoders are not a dependency. `base_room_policy` uses the suggested
  component ID 0x0027.
- Room-bound Chat restrictions are pure functions over `Chat.extra`
  (`may_update_room_chat`, `may_create_room_message`,
  `may_update_room_message`) that a `ChatBackend` returns from its
  per-Chat hooks (bd:JMAP-dfo4); this crate does not depend on
  `jmap-chat-server`.
