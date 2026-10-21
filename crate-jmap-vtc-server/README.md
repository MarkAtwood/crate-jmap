# jmap-vtc-server

Method handlers for the `VTCCall`, `VTCParticipant`, `VTCRecording` and
`VTCLivestream` data types of the JMAP VTC extension
(draft-atwood-jmap-vtc-00), backend-agnostic, for a
`jmap-server` `Dispatcher`. You supply storage through the `VtcBackend`
trait.

## What it is

| Item | Spec |
|---|---|
| `handle_vtc_call_get`, `handle_vtc_call_changes` | §VTCCall/get, §VTCCall/changes (RFC 8620 §5.1, §5.2) |
| `handle_vtc_call_set` create | §Creating a Ring Call, §Creating a Room Call, §Creating a Scheduled Call |
| `handle_vtc_call_set` update | §State Machine, §Ending a Call, §Moderator Actions on VTCCall |
| `handle_vtc_call_query`, `handle_vtc_call_query_changes` | §VTCCall/query, §VTCCall/queryChanges |
| `call_already_active_on_chat` | §VTCCall/set: at most one active call per Chat |
| `query::call_matches`, `query::compare_calls` | §VTCCall/query filter and sort |
| `handle_vtc_participant_set` create | §Joining a Room or Scheduled Call, §Gateway Dial-Out |
| `handle_vtc_participant_set` update | §Answering, §Declining, §Updating Media State, §Leaving, §Reconnecting, §Participant-Level Moderator Actions, §VTCGatewaySignal `pendingSignal` |
| `handle_vtc_participant_get`, `_changes`, `_query`, `_query_changes` | §VTCParticipant/get, /changes, /query, /queryChanges |
| `query::participant_matches`, `query::compare_participants` | §VTCParticipant/query filter and sort |
| `handle_vtc_recording_*` | §VTCRecording Methods: get, changes, set, query, queryChanges |
| `handle_vtc_livestream_*` | §VTCLivestream Methods, with §Livestream Key Exposure in `/get` |
| `query::recording_matches`, `query::livestream_matches`, `compare_*` | §VTCRecording/query, §VTCLivestream/query filter and sort |
| `VtcBackend` | storage, capability object, caller userId, moderator hook, recording consent event, unmute-request and gateway-signal delivery |
| `memory::MemoryBackend` (`memory` feature) | in-memory reference backend |
| `register_vtc_handlers` | wires the twenty `VTCCall/*`, `VTCParticipant/*`, `VTCRecording/*` and `VTCLivestream/*` methods |

## What it's for

A JMAP server that offers voice and video calls wires these handlers next
to its other extensions. The crate depends on `jmap-types`, `jmap-server`
and `jmap-vtc-types` only. The Chat binding (`chatId` membership,
`start_call`, `Chat.activeCallId`) goes in a separate binding crate.

## How to use

```rust,ignore
use std::sync::Arc;
use jmap_server::Dispatcher;
use jmap_vtc_server::{memory::MemoryBackend, register_vtc_handlers};

let backend = Arc::new(MemoryBackend::new());
let mut dispatcher: Dispatcher<String> = Dispatcher::new();
register_vtc_handlers(&mut dispatcher, backend);
```

Advertise `urn:ietf:params:jmap:vtc` in your Session with the same
account-level object that `VtcBackend::vtc_capability` returns.

## How it works

The handlers do the draft's validation, then call the backend. A create is
checked against the capability object (`mayCreateCall`,
`supports*Calls`, `supportedMediaTypes`), the type-specific required
arguments and the companion capabilities. It is then passed to
`VtcBackend::create_call`, which assigns the ULID `id` and the `joinUri`,
refuses a second active call on a Chat, creates the ring participants and
moves a ring call from `creating` to `ringing`, all in one atomic step.
An update may only request `"ended"`, and only from `ringing`, `active`
or `pending`. The handler adds `endedAt` and an `endReason` of
`completed` or `cancelled`. Ending a call, `lobbyEnabled`, `policy`,
`e2eeEnabled` and breakout rooms are gated on `VtcBackend::is_moderator`.
Every `VTCRecording/set` and `VTCLivestream/set` operation needs
`is_moderator` on the object's call. A create also needs
`supportsRecording` / `supportsLivestream`, a call that is not
`e2eeEnabled`, and a call in state `"active"`; `VtcBackend::create_object`
rechecks `"active"` atomically with the insert. Recordings move
`recording` ⇄ `paused` → `stopped` only, and each change (and the create)
is followed by `VtcBackend::deliver_recording_state_event`. A livestream
can only be stopped, from `starting` or `live`, and its `streamUri`,
`streamKey` and `platform` change only while `stopped` or `failed`.
`VTCLivestream/get` returns `streamKey` and `streamUri` as `""` to callers
who are not moderators on the livestream's call.
`VTCParticipant/set` checks each update against the caller (own record
or moderator) and the call policy, adds the server-set times
(`joinedAt`, `leftAt`, `kickedBy`), then hands it to
`VtcBackend::update_object`; the call transitions it causes (answer,
join, last decline, last departure, moderator promotion) and its event
deliveries run after that commits. `VtcBackend::create_object` and
`update_object` keep `activeParticipantCount` and refuse `alreadyExists`
and `overQuota` atomically.
The backend trait is not object-safe (generic methods); every public type
is `#[non_exhaustive]`.

## Gotchas

- `VtcBackend::is_moderator` defaults to `Ok(true)` (fail-open,
  bd:JMAP-bghk). Override it, or every caller can end any call.
- The one-active-call-per-Chat rule is enforced by `create_call`, not by
  the handler, so that the check is atomic with the insert.
- The `MemoryBackend` caller context is the caller's userId `String`. Its
  `/queryChanges` always returns `cannotCalculateChanges`.
- The draft does not say where a `null` `endedAt` sorts. `compare_calls`
  puts calls that have not ended last.
- `is_moderator`'s fail-open default also means every caller sees
  `streamKey` and may start, stop and delete recordings and livestreams.
- A call with `e2eeEnabled: true` can never be recorded or streamed
  through these handlers: the draft's key-holding recorder or bridge is
  not implemented. Register your own `/set` handler if you run one.
- `deliver_recording_state_event` has no default: the draft calls it
  "the consent signal". It runs after the change commits, so a failed
  delivery does not undo the change.
- The draft does not say where a livestream with a `null` `startedAt`
  sorts. `compare_livestreams` puts it last.
- With `is_moderator`'s fail-open default every joiner is a moderator:
  it bypasses the lobby and the call policy and may kick, admit and dial
  out.
- A join without `mediaState` gets the draft example's defaults (audio
  and video on) less what the policy keeps off. The handler sets
  `displayName` to the userId; the backend may replace it.
- The `answered_elsewhere` fork to the answerer's other devices, the ring
  timeout, and per-call access and E2EE participant visibility are not
  here (bd:JMAP-8mem.7, bd:JMAP-8mem.5).

## References

- [draft-atwood-jmap-vtc-00] — JMAP VTC
- [RFC 8620] — JMAP Core

[draft-atwood-jmap-vtc-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
