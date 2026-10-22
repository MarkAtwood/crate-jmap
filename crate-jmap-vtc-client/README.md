# jmap-vtc-client

## What it is

Typed client methods for the JMAP VTC extension ([draft-atwood-jmap-vtc-00]). Wraps
[`jmap-base-client`] transport with request builders and typed responses for all 20
VTC method names.

## What it's for

Implements the draft's `VTCCall/*`, `VTCParticipant/*`, `VTCRecording/*` and
`VTCLivestream/*` methods (each `/get`, `/changes`, `/set`, `/query`, `/queryChanges`)
on top of `jmap-base-client`. Sibling of `jmap-mail-client` in the extension-client
family. Wire types come from `jmap-vtc-types`.

## How to use

```rust,no_run
use jmap_base_client::{BearerAuth, ClientConfig, JmapClient};
use jmap_vtc_client::JmapVtcExt;
use jmap_vtc_types::{VtcParticipantFilter, VtcParticipantFilterCondition};
use jmap_types::Id;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let auth = BearerAuth::new("my-token")?;
let client = JmapClient::new_plain(auth, "https://jmap.example.com", ClientConfig::default())?;
let session = client.fetch_session().await?;
let sc = client.with_vtc_session(session);

// Ring Alice (draft §Creating a Ring Call).
let created = sc
    .vtc_call_set(
        Some(serde_json::json!({
            "c0": {
                "callType": "ring",
                "targetParticipantIds": ["user:alice@example.com"],
                "mediaTypes": ["audio", "video"]
            }
        })),
        None,
        None,
        None,
    )
    .await?;

// List who is in a call hosted in Bob's account, which this session
// lists as a shared account (draft §Hosting Account).
let hosted = sc.for_account("bob-account")?;
let mut cond = VtcParticipantFilterCondition::default();
cond.call_id = Some(Id::from("01J4XKZQN4MWVT8PPBEHTJ3AB0"));
cond.is_active = Some(true);
let ids = hosted
    .vtc_participant_query(
        Some(&VtcParticipantFilter::Condition(cond)),
        None,
        &jmap_base_client::QueryParams::default(),
    )
    .await?;
# Ok(())
# }
```

## Methods

All `pub async fn` on `SessionClient`. `<T>` is `vtc_call`, `vtc_participant`,
`vtc_recording` or `vtc_livestream`; the JMAP method prefix is `VTCCall`,
`VTCParticipant`, `VTCRecording` or `VTCLivestream`.

| Method | JMAP method | Returns |
|---|---|---|
| `<T>_get(ids, properties)` | `…/get` | `GetResponse<VtcCall>` (etc.) |
| `<T>_changes(since_state, max_changes)` | `…/changes` | `ChangesResponse` |
| `<T>_set(create, update, destroy, if_in_state)` | `…/set` | `SetResponse<PartialObject>` |
| `<T>_query(filter, sort, &QueryParams)` | `…/query` | `QueryResponse` |
| `<T>_query_changes(since_query_state, max_changes, filter, sort, up_to_id, calculate_total)` | `…/queryChanges` | `QueryChangesResponse` |

## How it works

Each method targets the primary account for `urn:ietf:params:jmap:vtc`, or the account
chosen with `SessionClient::for_account`, builds the
RFC 8620 §5 arguments, sends one invocation with
`using = ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:vtc"]`, and extracts the
typed result. Empty state tokens and an all-`None` `/set` fail with `InvalidArgument`
before any I/O.

## Gotchas

- Every method on a call's records goes to the call's hosting account
  (draft §Hosting Account), which for a call someone else started is a shared
  account, not the user's personal one. Answer, mute and leave through
  `sc.for_account(hosting_account_id)?`; to list all of a user's calls, query each
  VTC account in the session.

- `/set` returns `SetResponse<PartialObject>`: RFC 8620 §5.3 `created` entries carry only
  the server-set properties, which are not whole VTC objects.
- `/get` decodes whole objects; a `properties` subset that drops a required field fails
  to decode.
- The ring, in-call and gateway events are WebSocket/push payloads, not methods. Receive
  them over the WebSocket with `VtcWsExt` (below); decode push payloads with
  `jmap_vtc_types::VtcCallPush`.
- A `VTCStreamEnable` subscription dies with its connection. After reconnecting, send
  `send_vtc_stream_enable` again (draft-atwood-jmap-vtc-wss-00 §Subscription Lifecycle).

## VTC WebSocket events

`VtcWsExt` adds [draft-atwood-jmap-vtc-wss-00] ephemeral VTC events to a
`jmap_base_client::WsSession`. `send_vtc_stream_enable` starts or replaces the
subscription and `send_vtc_stream_disable` cancels it. `next_vtc_frame` yields the eight
VTC event types as `VtcWsFrame::Event(VtcEvent)`. Every other frame (`StateChange`,
`Response`, a `RequestError` rejecting a `VTCStreamEnable`, Chat or Scene events,
unrecognized `@type`s, and VTC events that do not parse) arrives unchanged as
`VtcWsFrame::Other`.

```rust,no_run
use jmap_base_client::WsSession;
use jmap_vtc_client::{VtcWsExt, VtcWsFrame};
use jmap_vtc_types::{VtcEvent, VtcStreamEnable};

# async fn run(mut ws: WsSession) -> Result<(), jmap_base_client::ClientError> {
let mut enable = VtcStreamEnable::default();
enable.call_ids = Some(vec!["01J4XKZQN4MWVT8PPBEHTJ3AB0".into()]);
ws.send_vtc_stream_enable(&enable).await?;
while let Some(frame) = ws.next_vtc_frame().await {
    if let VtcWsFrame::Event(VtcEvent::Participant(e)) = frame? {
        println!("{} {:?}", e.display_name, e.event);
    }
}
# Ok(()) }
```

## References

- **[draft-atwood-jmap-vtc-00]** — JMAP VTC
- **[draft-atwood-jmap-vtc-wss-00]** — JMAP VTC WebSocket extension
- **[RFC 8620]** — JMAP Core

[draft-atwood-jmap-vtc-00]: https://github.com/MarkAtwood/jmap-chat-spec
[draft-atwood-jmap-vtc-wss-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-base-client`]: ../crate-jmap-base-client
