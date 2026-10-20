# jmap-vtc-types — Plan

Typed surface of draft-atwood-jmap-vtc-00 (epic bd:JMAP-8mem). Types only:
the handlers live in the planned `jmap-vtc-server`, the method bindings in
`jmap-vtc-client`.

## Scope

| Bead | Content | State |
|---|---|---|
| JMAP-8mem.1 | Capability, VTCGateway, VTCDialInEntry, VTCMediaState, VTCCallPolicy, the four data types, VTCCallPush, the eight events | landed |

## Decisions

- The session-level capability value `{}` is not modelled, as in
  `jmap-chat-mimi-types`.
- `T|null` properties are `Option<T>` and serialize `None` as `null`, so a
  full object round-trips with its nulls.
- Properties that may be absent *or* `null` with different meanings are
  `Option<Option<T>>` (absent `None`, `null` `Some(None)`): the VTCCall
  binding fields (present only with the companion capability),
  `maxBreakoutRooms` (absent when breakout rooms are unsupported, `null`
  for no limit), and the nullable VTCCallPush fields the content-free form
  drops.
- `VTCCall.accountId` is optional: the draft's own `VTCCall/get` example
  omits it and the `/get` response carries `accountId` anyway.
- VTCCallPush is one struct for both forms; a missing `initiatorId` marks
  the content-free form. Its `@type` is required and checked.
- Events carry no `@type` themselves; `VtcEvent` adds it and dispatches on
  it, with `Unknown` for unrecognised types, as `jmap-chat-types`
  `EphemeralMessage` does.
- userId-valued properties (`initiatorId`, `userId`, `kickedBy`,
  `initiatedBy`, `requestedBy`) are `String`, not `Id`: the draft defines
  userId as the transport-layer identity string, e.g.
  `"user:bob@example.com"`.
- `joinMethod`, gateway `protocol`, media types and every state or reason
  are result enums with `Other(String)`. `platform` and gateway `signal`
  stay `String`: the draft calls them deployment- or protocol-defined.
- Documented, not enforced: `supportedMediaTypes` includes `"audio"`;
  VTCRecordingStateEvent carries exactly one of `recordingId` and
  `livestreamId`.
- `Debug` redacts `joinPassword`, `gatewayPin`, `streamUri` and
  `streamKey`.
