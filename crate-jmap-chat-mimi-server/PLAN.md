# jmap-chat-mimi-server — Plan

Server rules of draft-atwood-jmap-chat-mimi-00 (epic bd:JMAP-6k5d), on top
of `jmap-chat-mimi-types`.

## Scope

| Bead | Content | State |
|---|---|---|
| JMAP-6k5d.2 | Crate scaffold, `MimiBackend`, KeyPackage/get and /set, `memory` backend | landed |
| JMAP-6k5d.3 | KeyPackage hand-out: atomic claim, `clientUri`, ref retention | open |
| JMAP-6k5d.4 | Room/create checks and bound Chat restrictions | open |
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
