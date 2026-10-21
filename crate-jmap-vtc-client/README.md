# jmap-vtc-client

> **Draft, not published.** Tracks a draft spec that is still changing
> (jmap-chat-spec 6bcd9d8 revised it); `publish = false` until the crate
> matches it (JMAP-0x0q, JMAP-38uq.14).

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
use jmap_vtc_types::query::{VtcParticipantFilter, VtcParticipantFilterCondition};
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
    )
    .await?;

// List who is in a call hosted in Bob's account, which this session
// lists as a shared account (draft §Hosting Account).
let hosted = sc.for_account("bob-account")?;
let mut cond = VtcParticipantFilterCondition::default();
cond.call_id = Some(Id::from("01J4XKZQN4MWVT8PPBEHTJ3AB0"));
cond.is_active = Some(true);
let ids = hosted
    .vtc_participant_query(Some(&VtcParticipantFilter::Condition(cond)), None, None, None)
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
| `<T>_set(create, update, destroy)` | `…/set` | `SetResponse` |
| `<T>_query(filter, sort, position, limit)` | `…/query` | `QueryResponse` |
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

- `/set` returns the untyped `SetResponse`: RFC 8620 §5.3 `created` entries carry only
  the server-set properties, which are not whole VTC objects.
- `/get` decodes whole objects; a `properties` subset that drops a required field fails
  to decode.
- The ring, in-call and gateway events are WebSocket/push payloads, not methods. Decode
  them with `jmap_vtc_types::VtcEvent` and `VtcCallPush`.

## References

- **[draft-atwood-jmap-vtc-00]** — JMAP VTC
- **[RFC 8620]** — JMAP Core

[draft-atwood-jmap-vtc-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-base-client`]: ../crate-jmap-base-client
