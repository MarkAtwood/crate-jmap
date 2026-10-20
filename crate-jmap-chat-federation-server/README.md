# jmap-chat-federation-server

Server rules for draft-atwood-jmap-chat-federation-00, server-to-server
delivery between JMAP Chat mailboxes: peer-role access control and the
`Peer/deliver` handler. No transport and no network I/O; the deployment
authenticates requests and fetches blobs.

## What it is

| Item | Spec |
|---|---|
| `FederationCaller` | §Account-Level Capability: a `CallerCtx` says whether the caller is a peer on a `role: "peer"` account |
| `PEER_METHODS` | §Server-to-Server Methods: the eight `Peer/*` method names |
| `check_method_access` | §Account-Level Capability and §Server-to-Server Methods: `forbidden` for a peer calling a non-`Peer/*` method, and for anyone else calling a `Peer/*` method |
| `handle_peer_deliver` | §Peer/deliver: §New Message Validation steps 1 to 14, §Edit and Reaction Validation, §Response, §Direct Chat Simultaneous Initiation |
| `FederationBackend` | A `ChatBackend` that also supplies the `AuthenticatedPeer`, the `@everyone` permission floor, attachment fetching (`FetchedBlob`), fresh Message ids, and direct Chats under a peer-chosen id |

## What it's for

A federating chat server uses it to keep peer servers on the `Peer/*`
methods and local users off them, and to accept messages, edits and
reactions that peers deliver.

## How to use

```rust
use jmap_chat_federation_server::{check_method_access, FederationCaller};

struct Caller { peer: bool }

impl FederationCaller for Caller {
    fn is_peer(&self) -> bool { self.peer }
}

assert!(check_method_access(&Caller { peer: false }, "Peer/deliver").is_err());
```

`handle_peer_deliver(&backend, &caller, args)` has the signature of the
`jmap-chat-server` handlers, so it registers with a `ClosureHandler`.

## How it works

- `check_method_access` compares `is_peer()` with whether the method is in
  `PEER_METHODS`; a mismatch is `forbidden`.
- `handle_peer_deliver` checks the identity binding and `senderUserId`,
  resolves the Chat, applies SpaceBan, `send`, slow mode and blocking,
  validates the payload, then fetches attachments and writes. Every
  rejection happens before the first write, so a rejected request stores
  nothing and creates no placeholder ChatContact.
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
- `FederationBackend::fetch_attachment` must apply §Blob Fetch and SSRF to
  the peer-supplied `fetchUrl`; the handler does not see the URL's target.

## References

- [draft-atwood-jmap-chat-federation-00] — JMAP Chat federation

[draft-atwood-jmap-chat-federation-00]: https://github.com/MarkAtwood/jmap-chat-spec
