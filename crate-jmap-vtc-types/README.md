# jmap-vtc-types

> **Draft, not published.** Tracks a draft spec that is still changing
> (jmap-chat-spec 63e9d08 revised it); `publish = false` until the crate
> matches it (JMAP-0x0q, JMAP-38uq.14).

Wire-format types for draft-atwood-jmap-vtc-00, the `urn:ietf:params:jmap:vtc`
capability that manages voice and video call state over JMAP. Types only:
no method handlers, no I/O, no media.

## What it is

| Item | Spec |
|---|---|
| `JMAP_VTC_URI` | §Capability |
| `VtcCapability` | §Account-Level Capability Object |
| `VtcGateway`, `VtcDialInEntry`, `GatewayProtocol` | §VTCGateway, §VTCDialInEntry |
| `MediaKind` | §Account-Level Capability Object (`supportedMediaTypes`) |
| `VtcMediaState` | §VTCMediaState |
| `VtcCallPolicy` | §VTCCallPolicy |
| `VtcCall`, `CallType`, `CallState`, `CallEndReason` | §VTCCall, §State Machine |
| `VtcParticipant`, `CallRole`, `CallResponse`, `LobbyState`, `JoinMethod` | §VTCParticipant |
| `VtcRecording`, `RecordingState` | §VTCRecording |
| `VtcLivestream`, `LivestreamState` | §VTCLivestream |
| `VtcCallPush` | §Push: VTCCallPush, §Content-Free Form |
| `VtcRingEvent` | §WebSocket: VTCRingEvent |
| `VtcCallEndEvent`, `RingEndReason` | §VTCCallEndEvent |
| `VtcGatewaySignal`, `SignalDirection` | §VTCGatewaySignal |
| `VtcParticipantEvent`, `ParticipantEventKind`, `VtcMediaStateEvent`, `VtcActiveSpeakerEvent`, `VtcUnmuteRequestEvent`, `VtcRecordingStateEvent` | §In-Call Ephemeral Events |
| `VtcEvent` | `@type` dispatch over the eight events |

The session-level capability value is `{}` and has no type.

## What it's for

It gives the planned `jmap-vtc-server` and `jmap-vtc-client`, and any
consumer that relays VTC pushes or WebSocket events, one definition of the
VTC wire types. It depends on `jmap-types` only; the Chat, Space and
CalendarEvent ids it carries are plain `Id`s.

## How to use

```rust
use jmap_vtc_types::{RingEndReason, VtcEvent};

let ev: VtcEvent = serde_json::from_str(r#"{
    "@type": "VTCCallEndEvent",
    "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB",
    "endReason": "answered_elsewhere"
}"#)?;
let VtcEvent::CallEnd(end) = ev else { unreachable!() };
assert_eq!(end.end_reason, RingEndReason::AnsweredElsewhere);
# Ok::<(), serde_json::Error>(())
```

## How it works

- `T|null` properties are `Option<T>` and serialize `None` as `null`.
  Properties whose absence and `null` mean different things are
  `Option<Option<T>>`: `None` absent, `Some(None)` `null`.
- `VtcCallPush` and `VtcEvent` emit `@type` on serialize and consume it on
  deserialize; it never lands in `extra`.
- `#[non_exhaustive]` on every public struct and enum, an `extra` map for
  vendor fields, and `Other(String)` on every wire enum, per workspace
  policy.
- No async. `#[forbid(unsafe_code)]` at the crate root.

## Gotchas

- The VTCCall binding fields `chat_id`, `space_id`, `channel_id` and
  `calendar_event_id` are `Option<Option<Id>>`. `None` means the server
  does not advertise the companion capability; `Some(None)` means the call
  is unbound.
- `VtcCallPush` covers both forms. Check `initiator_id.is_none()` to detect
  the content-free form, then fetch the call.
- `VtcCall::account_id` is optional because the draft's `VTCCall/get`
  example omits it.
- `joinUri` and `streamUri` are peer-supplied and untrusted. Nothing here
  validates them.
- `Debug` redacts `joinPassword`, `gatewayPin`, `streamUri` and
  `streamKey`; `Serialize` does not.
- Not enforced on deserialize: `supportedMediaTypes` containing `"audio"`,
  and exactly one of `recordingId` / `livestreamId` on
  `VtcRecordingStateEvent`.

## References

- [draft-atwood-jmap-vtc-00] — JMAP VTC
- [RFC 8620] — JMAP Core

[draft-atwood-jmap-vtc-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
