# jmap-chat-spec patches for Mark's review (2026-11-19)

Against jmap-chat-spec 120db2c. Not committed there: apply with
`git -C /data/PROJECT/jmap-chat-spec apply <patch>` once approved.

| Patch | Bead | What it changes |
|---|---|---|
| JMAP-ugi1.patch | JMAP-ugi1 | jmap-chat-compliance-tests.md: §6.3 tests spaceId/spaceName only when requested in `properties` (they are not in the push draft's default set); §12.1 drops `PushSubscription/changes`, which RFC 8620 §7.2 does not define; §13.1 notes it repeats §3.1 |
| JMAP-38uq.17.patch | JMAP-38uq.17 | draft-atwood-jmap-scene-00: the three leave/eject responses echo the server-recorded `leftAt` (RFC 8620 §5.3 "changed in a way not explicitly requested"), as jmap-scene-server does |
| JMAP-cklk-595m.patch | JMAP-cklk, JMAP-595m | draft-atwood-jmap-vtc-00: `lobbyState` is always present and `null` without a lobby (cklk 3); the VTCCall/get example gains `accountId` (cklk 4); the capability gates get errors: `lobbyEnabled` without `supportsLobby` and `parentCallId` without `supportsBreakoutRooms` are `invalidProperties`, `maxConcurrentCalls` and `maxBreakoutRooms` are `overQuota` on create (595m 1, 2); a `null` `endedAt` sorts after ended calls (595m 3, as jmap-vtc-server does); the scheduled example says it is sent before its start (595m 4) |

Already fixed upstream since the beads were filed, so not in the patches:
cklk 1 (VTCParticipant `id` is now a ULID, so the Id charset holds) and
cklk 2 (the VTCCallPush example has `channelId`).

ugi1's "reconcile §3.1/§13.1 with §3.2-3.5" was about the x3lu audit's
classification (the testjig covers 3.1; 3.2-3.5 check a Session the
consumer builds), not the compliance doc; the patch only marks 13.1 as a
repeat of 3.1.

After approval, jmap-vtc-server needs the 595m gates (JMAP-595m's
"then implement" half); the scene and compliance patches need no crate
change.
