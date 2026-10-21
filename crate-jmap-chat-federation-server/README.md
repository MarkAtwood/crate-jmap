# jmap-chat-federation-server

Server rules for draft-atwood-jmap-chat-federation-00, server-to-server
delivery between JMAP Chat mailboxes: peer-role access control and the
`Peer/deliver`, `Peer/receipt`, `Peer/typing`, `Peer/retract`,
`Peer/groupUpdate` and optional presence handlers. No transport and no network I/O; the deployment
authenticates requests and fetches blobs.

## What it is

| Item | Spec |
|---|---|
| `FederationCaller` | §Account-Level Capability: a `CallerCtx` says whether the caller is a peer on a `role: "peer"` account |
| `PEER_METHODS` | §Server-to-Server Methods: the eight `Peer/*` method names |
| `check_method_access` | §Account-Level Capability and §Server-to-Server Methods: `forbidden` for a peer calling a non-`Peer/*` method, and for anyone else calling a `Peer/*` method |
| `handle_peer_deliver` | §Peer/deliver: §New Message Validation steps 1 to 14, §Edit and Reaction Validation, §Response, §Direct Chat Simultaneous Initiation |
| `handle_peer_receipt` | §Peer/receipt: the reader's receipt in `deliveryReceipts`, `deliveryState`, the `receiptSharing` gate of "Receiver Behavior" |
| `handle_peer_typing` | §Peer/typing: membership, `blocked` and `receiveTypingIndicators` checks, then `forward_typing` |
| `handle_peer_retract` | §Peer/retract: the original-sender check and the tombstone |
| `handle_peer_group_update` | §Peer/groupUpdate: `create`, `addMembers`, `removeMembers`, `updateRoles`, `updateMetadata`, `leave`, `close`, with the admin and `closedAt` checks |
| `handle_peer_subscribe_presence`, `handle_peer_unsubscribe_presence` | §Peer/subscribePresence, §Peer/unsubscribePresence: `subscriberId` check, owner presence with `"invisible"` as `"offline"`, granted TTL |
| `handle_peer_presence` | §Peer/presence "Receiver Behavior": ChatContact `presence`, `lastActiveAt`, `statusText`, `statusEmoji` update, blocked contacts dropped silently, then `forward_presence` |
| `FederationPresenceBackend` | A `FederationBackend` that also keeps presence subscriptions and forwards presence events to the owner's clients; only deployments offering the optional methods implement it |
| `FederationBackend` | A `ChatBackend` that also supplies the `AuthenticatedPeer`, the `@everyone` permission floor, attachment fetching (`FetchedBlob`), fresh Message ids, Chats under a peer-chosen id, and typing events forwarded to the owner's clients |

## What it's for

A federating chat server uses it to keep peer servers on the `Peer/*`
methods and local users off them, and to accept what peers send:
messages, edits, reactions, receipts, typing, retractions and group
changes.

## How to use

```rust
use jmap_chat_federation_server::{check_method_access, FederationCaller};

struct Caller { peer: bool }

impl FederationCaller for Caller {
    fn is_peer(&self) -> bool { self.peer }
}

assert!(check_method_access(&Caller { peer: false }, "Peer/deliver").is_err());
```

Every `handle_peer_*(&backend, &caller, args)` has the signature of the
`jmap-chat-server` handlers, so it registers with a `ClosureHandler`.

## How it works

- `check_method_access` compares `is_peer()` with whether the method is in
  `PEER_METHODS`; a mismatch is `forbidden`.
- `handle_peer_deliver` checks the identity binding and `senderUserId`,
  resolves the Chat, applies SpaceBan, `send`, slow mode and blocking,
  validates the payload, then fetches attachments and writes. Every
  rejection happens before the first write, so a rejected request stores
  nothing and creates no placeholder ChatContact.
- Every handler checks its caller-identity field (`senderUserId` or
  `readerUserId`) against the authenticated peer first, and rejects before
  its first write.
- `handle_peer_receipt` finds the owner's Message by `senderMsgId` and
  requires the reader to be in its Chat. With `receiptSharing` off it
  keeps only `deliveredAt` and answers as usual.
- `handle_peer_group_update` resolves `members` and `addedMembers` with
  placeholder ChatContacts; `removedMemberIds` and `updatedRoles` act only
  on identities that are members.
- A caller whose account has an unknown `role` value is not a peer.
- `#[forbid(unsafe_code)]` at the crate root.

## Gotchas

- The `jmap-server` `Dispatcher` has no pre-handler hook, so handlers
  registered by `jmap_chat_server::register_chat_handlers` do not call
  `check_method_access`; a peer can still reach them (bd:JMAP-v9py.15.8).
- An owner calling an undefined `Peer/*` name is not `forbidden`; the
  dispatcher answers `unknownMethod`.
- `handle_peer_deliver` reads all Chats, ChatContacts, SpaceBans and
  Messages of the account per call, and its writes are separate backend
  calls. A backend failure between writes returns `serverFail`; the peer's
  retry reuses what was written.
- `FederationBackend::fetch_attachment` and `fetch_avatar` must apply
  §Blob Fetch and SSRF to the peer-supplied `fetchUrl` and fetch it with a
  GET signed per §Request Signing (tag `"jmap-chat-peer"`, covering
  `@method` and `@target-uri`) that follows no cross-origin redirect; the
  handler does not see the URL's target. `fetch_attachment` maps a
  connection failure, timeout or 5xx to `Err` (`serverFail`), a 429 to
  `rateLimited`, and a 403 or 404 to `invalidArguments`.
- Serving blobs at the `fetchUrl` values this server issues is the
  deployment's: verify each fetch with `jmap_peer_auth_io::authenticate`,
  serve the blob only to a peer domain the server sent a payload carrying
  that `fetchUrl` (recording the grant before sending the payload), answer
  any other verified peer with 404, and never redirect to another origin
  (§Blob Fetch).
- `FederationBackend::forward_typing` owns the `Peer/typing` rate limit
  (one call per peer per chat per 3 s, SHOULD); the handler counts nothing.
- The presence methods are optional. Without a `FederationPresenceBackend`,
  leave them unregistered so callers get `unknownMethod`.
- `handle_peer_presence` creates no ChatContact: an update for an identity
  with none is acknowledged and ignored.
- Sending `Peer/presence` to subscribers, its rate limit, and subscription
  expiry are the deployment's.
- `Peer/groupUpdate` fetches an `avatar` object through
  `FederationBackend::fetch_avatar`, best-effort: a failed avatar never
  rejects the request. `peer_group_update_args` takes the outbound
  `avatar` object; minting its `fetchUrl` and recording the grant are the
  deployment's.

## References

- [draft-atwood-jmap-chat-federation-00] — JMAP Chat federation

[draft-atwood-jmap-chat-federation-00]: https://github.com/MarkAtwood/jmap-chat-spec
