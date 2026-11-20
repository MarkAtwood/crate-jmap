# jmap-chat-spec patches for Mark's review (2026-11-19)

Against jmap-chat-spec 120db2c. Not committed there: apply with
`git -C /data/PROJECT/jmap-chat-spec apply <patch>` once approved.

| Patch | Bead | What it changes |
|---|---|---|
| JMAP-ugi1.patch | JMAP-ugi1 | jmap-chat-compliance-tests.md: §6.3 tests spaceId/spaceName only when requested in `properties` (they are not in the push draft's default set); §12.1 drops `PushSubscription/changes`, which RFC 8620 §7.2 does not define; §13.1 notes it repeats §3.1 |
| JMAP-38uq.17.patch | JMAP-38uq.17 | draft-atwood-jmap-scene-00: the three leave/eject responses echo the server-recorded `leftAt` (RFC 8620 §5.3 "changed in a way not explicitly requested"), as jmap-scene-server does |
| push-platform-guide-fcm-wns.patch | (from JMAP-xxnf) | jmap-push-platform-guide.md: FCM HTTP v1 errors carry an HTTP error status, and the FCM code (`UNREGISTERED`, ...) is in `error.details[].errorCode` of the `FcmError` entry, not `error.status`, which is the general Google status (`NOT_FOUND`); WNS's current status header is `X-WNS-Status`, accept the older `X-WNS-NotificationStatus` too. Found building jmap-platform-push against the provider docs (FCM v1 ErrorCode reference; Microsoft "Push notification service request and response headers") |
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

## Added 2026-11-20 (not yet applied)

| Patch | Bead | What it changes |
|---|---|---|
| scene-games-guide-permissions.patch (applied as jmap-chat-spec f2e88ed, 2026-11-20) | JMAP-8loa (spec-internal part) | jmap-scene-board-games-guide.md, against 3445973: the FPS network model now cites the draft's 5-30 s avatar reconciliation instead of "1-5 Hz"; health and inventory that the simulation owns move from SceneAvatar `worldState` (only the user may write it, Avatar Permissions) to per-player SceneObjects owned by the region owner; the Counter-Strike pattern takes per-player money out of public region `worldState` into those objects, hidden by object visibility filtering. Found reviewing project-cordite. |

