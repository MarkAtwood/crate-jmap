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
| `MediaEffect`, `VtcBackend::run_media_effect` | the media start, pause, resume, stop and layout change a stored VTCRecording/VTCLivestream change implies, requested after the write |
| `media_report::MediaReporter` | §Livestream States and §Pausing, Resuming, and Stopping a Recording server-initiated transitions: `livestream_live`, `livestream_endpoints`, `livestream_failed`, `livestream_stopped` (idle stop), `recording_paused`, `recording_resumed`, `recording_stopped`, `recording_processing`, `recording_available`, `recording_failed` |
| `memory::MemoryBackend` (`memory` feature) | in-memory reference backend; accounts with `with_account` / `register_account`, companion capabilities with `advertise` |
| `register_vtc_handlers` | wires the twenty `VTCCall/*`, `VTCParticipant/*`, `VTCRecording/*` and `VTCLivestream/*` methods |

## What it's for

A JMAP server that offers voice and video calls wires these handlers next
to its other extensions. The crate depends on `jmap-types`, `jmap-server`
and `jmap-vtc-types` only. The Chat binding (`chatId` membership,
`start_call`, `Chat.activeCallId`) goes in a separate binding crate.

## How to use

```rust,ignore
use std::sync::Arc;
use jmap_server::{Dispatcher, Id};
use jmap_vtc_server::{memory::MemoryBackend, register_vtc_handlers};

// `capability`: the account-level object the Session advertises.
let backend = Arc::new(MemoryBackend::new().with_account(&account_id, capability));
let mut dispatcher: Dispatcher<Id> = Dispatcher::new();
register_vtc_handlers(&mut dispatcher, backend);
```

Advertise `urn:ietf:params:jmap:vtc` in your Session with the same
account-level object that `VtcBackend::vtc_capability` returns.

### WebSocket ephemeral events (`websocket` feature)

`websocket::VtcStreamHandler` is a `jmap-websocket` extension handler for
`VTCStreamEnable` and `VTCStreamDisable` (draft-atwood-jmap-vtc-wss-00).
Keep one `VtcEventHub`, publish every VTC event to it, and register one
handler per connection next to advertising
`urn:ietf:params:jmap:vtc:websocket`:

```rust,ignore
use jmap_vtc_server::websocket::{VtcEventHub, VtcStreamHandler};

let hub = VtcEventHub::new(256); // server-wide
let vtc = VtcStreamHandler::new(Arc::clone(&backend), caller, own_account_id, &hub);
let connection = connection.with_extension(vtc);

hub.publish(participant_event);             // per-call: accountId = hosting account
hub.publish_to_user(callee_user_id, ring);  // per-user: VTCRingEvent, VTCCallEndEvent
```

- Each event goes through `should_deliver` with its `accountId` as the
  hosting account. A per-user event published without a user reaches no
  one.
- `VtcStreamLimits` (default: the draft's) keeps one
  `VTCMediaStateEvent` per participant per call per second, sending the
  latest held one when the window reopens, and two
  `VTCActiveSpeakerEvent`s per call per second.
- `chat_alias()` is the `ChatStreamEnable` `"vtc"` switch;
  `jmap-chat-vtc-server::link_vtc_stream` drives it.

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
is followed by `VtcBackend::deliver_recording_state_event`; pausing is
refused when the account's `supportsRecordingPause` is `false`. A livestream
create takes a `mode` from the account's `livestreamModes` (`push` by
default): push needs `streamKey` and a `streamUri` whose scheme is in
`livestreamPushSchemes`, and hosted takes only
`callId` and `mode` and starts with no `playbackEndpoints`. A livestream
can only be stopped, from `starting` or `live`; `stopped` and `failed` are
final, and `mode`, `streamUri`, `streamKey` and `platform` are immutable.
Stopping empties `playbackEndpoints`. Both types take an optional `layout`
from the account's `layouts`, changeable while a recording is `recording`
or `paused` and while a livestream is `starting` or `live`.
`create_object` and `update_object_if` only store: no media starts or
stops inside them, because an atomic conditional `/set` can still roll the
write back and nothing rolls back a started stream. After the write the
handler passes a `MediaEffect` (`StartLivestream`, `StopLivestream`,
`StartRecording`, `PauseRecording`, `ResumeRecording`, `StopRecording`,
`ChangeRecordingLayout`, `ChangeLivestreamLayout`, carrying the object as
stored) to `VtcBackend::run_media_effect`; ending
a call and destroying an active object request the stops too. The media
pipeline reports back through `media_report::MediaReporter`, which applies
each server-initiated transition with `update_object_if` from the draft's
source states, so state strings and change logs move as for a client
update, recording pauses and stops deliver the `VTCRecordingStateEvent`,
and a livestream that fails or is stopped by the server requests
`StopLivestream` so the pipeline is released.
Every `state` change goes through `VtcBackend::update_object_if`, which
checks the stored state against the transition's source states
atomically with the write, so concurrent updates cannot slip an unlisted
transition through.
`VTCLivestream/get` returns `streamKey` and `streamUri` as `""` to callers
who are not moderators on the livestream's call, and each listed
livestream's `playbackEndpoints` as `VtcBackend::playback_endpoints_for`
computes them for the caller (§Playback Authorization; the default returns
the stored list).
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

### Conditional Foo/set (draft-ietf-jmap-conditional-02)

`register_vtc_conditional(dispatcher, backend)`, called after
`register_vtc_handlers`, attaches `SetSupport` to each registered `/set`
(VTCCall, VTCParticipant, VTCRecording, VTCLivestream) and skips the rest; it
re-registers nothing, and a deployment that never calls it is unchanged
(docs/design/DECISIONS.md decisions 1 and 17). Advertise
`urn:ietf:params:jmap:conditional` only when `Dispatcher::conditional_ready()` is
true.

`VTCLivestream/get` marks the `streamKey` and `streamUri` it blanks for a
non-moderator as unreadable, so a precondition on either is `forbidden` (§9.1)
rather than compared with the blank. `MemoryBackend`'s store is a
`jmap_server::ScopedState`: an atomic rollback restores objects, states and
derived counts and drops queued `deliver_event` events and
`run_media_effect` effects. A backend of your own MUST hold media effects
requested inside an atomic scope until `SetScope::commit`. Under an atomic scope the
"one participant record per user per call" rule and `maxParticipantsPerCall` move
to the commit, which checks the final state, so a moderator can kick one
participant and admit another in one call at the cap.

## Object history (draft-ietf-jmap-object-history-03)

`register_vtc_handlers_with_history(dispatcher, backend)`, for a backend
that also implements `jmap_server::ObjectHistoryBackend`, calls
`register_vtc_handlers` and re-registers history-aware `/get`, `/set`,
`/query` and `/queryChanges` of VTCCall, VTCRecording and VTCLivestream.
VTCParticipant keeps no history (decision 22). Every VTCLivestream version
gets the same `streamKey`/`streamUri` blanking as the current one for a
non-moderator (§8.2). A restored recording or livestream needs a current
call; a restored breakout room, its parent call. History reads emit no VTC
events; a restore emits what an update would. The memory backend's
`with_object_history(capability, retention)` keeps the versions and
evaluates no filter or sort over destroyed objects.

## Gotchas

- `VtcBackend::is_moderator` defaults to `Ok(true)` (fail-open,
  bd:JMAP-bghk). Override it, or every caller can end any call.
- The one-active-call-per-Chat rule is enforced by `create_call`, not by
  the handler, so that the check is atomic with the insert.
- The `MemoryBackend` caller context is a `jmap_server::MemoryCaller`: the
  caller's Principal `Id` (whose string is the userId), or `()` (the
  account id is the userId). Its `/queryChanges` always returns `cannotCalculateChanges`.
- The draft does not say where a `null` `endedAt` sorts. `compare_calls`
  puts calls that have not ended last.
- `is_moderator`'s fail-open default also means every caller sees
  `streamKey` and may start, stop and delete recordings and livestreams.
- A call with `e2eeEnabled: true` can never be recorded or streamed
  through these handlers: the draft's key-holding recorder or bridge is
  not implemented. Register your own `/set` handler if you run one.
- `run_media_effect` defaults to doing nothing, so without an override a
  livestream stays `"starting"` and a recording never becomes
  `"available"`. It runs after the write but, inside a conditional
  `/set` scope, before the commit: queue the effect in the scope and run
  it at commit. Stops can repeat (a failure report, then the call's end),
  so make them idempotent. An object-history restore of a livestream or
  recording requests no media effect.
- `MediaReporter` reports run as the context you give it; recording
  events it delivers name that context's userId as `initiatedBy`.
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
