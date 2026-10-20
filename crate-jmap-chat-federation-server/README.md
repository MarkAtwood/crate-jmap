# jmap-chat-federation-server

Server rules for draft-atwood-jmap-chat-federation-00, server-to-server
delivery between JMAP Chat mailboxes. No handlers yet, no transport, no
I/O.

## What it is

| Item | Spec |
|---|---|
| `FederationCaller` | §Account-Level Capability: a `CallerCtx` says whether the caller is a peer on a `role: "peer"` account |
| `PEER_METHODS` | §Server-to-Server Methods: the eight `Peer/*` method names |
| `check_method_access` | §Account-Level Capability and §Server-to-Server Methods: `forbidden` for a peer calling a non-`Peer/*` method, and for anyone else calling a `Peer/*` method |

## What it's for

A federating chat server uses it to keep peer servers on the `Peer/*`
methods and local users off them.

## How to use

```rust
use jmap_chat_federation_server::{check_method_access, FederationCaller};

struct Caller { peer: bool }

impl FederationCaller for Caller {
    fn is_peer(&self) -> bool { self.peer }
}

assert!(check_method_access(&Caller { peer: false }, "Peer/deliver").is_err());
```

## How it works

- `check_method_access` compares `is_peer()` with whether the method is in
  `PEER_METHODS`; a mismatch is `forbidden`.
- A caller whose account has an unknown `role` value is not a peer.
- `#[forbid(unsafe_code)]` at the crate root.

## Gotchas

- The `jmap-server` `Dispatcher` has no pre-handler hook, so handlers
  registered by `jmap_chat_server::register_chat_handlers` do not call
  `check_method_access`; a peer can still reach them (bd:JMAP-v9py.15.8).
- An owner calling an undefined `Peer/*` name is not `forbidden`; the
  dispatcher answers `unknownMethod`.

## References

- [draft-atwood-jmap-chat-federation-00] — JMAP Chat federation

[draft-atwood-jmap-chat-federation-00]: https://github.com/MarkAtwood/jmap-chat-spec
