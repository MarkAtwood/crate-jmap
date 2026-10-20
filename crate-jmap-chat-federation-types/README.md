# jmap-chat-federation-types

Wire-format types for draft-atwood-jmap-chat-federation-00, server-to-server
delivery between JMAP Chat mailboxes. Types only: no I/O, no transport.

## What it is

| Item | Spec |
|---|---|
| `contact_server_url` | §ChatContact Type Extensions for Federation (`serverUrl`) |
| `contact_direct_address` | §ChatContact Type Extensions for Federation (`directAddress`), §Direct Address Hints |
| `peer::Peer*Args`, `peer::Peer*Response`, `peer::PeerAck` | §Server-to-Server Methods: `Peer/deliver`, `Peer/receipt`, `Peer/typing`, `Peer/retract`, `Peer/groupUpdate`, and the optional `Peer/subscribePresence`, `Peer/unsubscribePresence`, `Peer/presence` |
| `PeerMention`, `PeerChatMember`, `PeerRoleUpdate`, `PeerAttachment` | §User References in Peer Methods (identity strings, not ChatContact ids), §Peer/deliver (`fetchUrl`) |
| `FederationSession` | §Session Object Extensions for Federation (`ownerUserId`, `peerSigningKeys`, `ownerDirectAddress`) |
| `anonymous_well_known_session` | §Key Publication (the Session answered to an unsigned GET of `/.well-known/jmap`) |

## What it's for

It gives federating chat servers and their clients one definition of the
federation extension's wire types. It sits beside `jmap-chat-types` and does
not change it: federation fields on chat objects stay in their `extra` maps
and are read from there.

## How to use

```rust
use jmap_chat_federation_types::contact_server_url;

let extra = serde_json::json!({ "serverUrl": "https://b.example/jmap/" });
assert_eq!(
    contact_server_url(extra.as_object().unwrap()),
    Some("https://b.example/jmap/")
);
```

With a `jmap_chat_types::ChatContact`, pass `&contact.extra`.

## How it works

- Each reader looks up one key in the `extra` map and returns the string
  verbatim, or `None` when the key is absent or not a string.
- The `Peer/*` types are plain serde structs: camelCase on the wire, an
  `extra` map for unknown fields, a `new` taking the required fields.
  `ReactionAction` and `GroupUpdateAction` reject unknown strings.
- `FederationSession::from_session_extra` reads the Session fields from a
  parsed Session's extension map. `peerSigningKeys` entries stay raw JSON:
  filter them with `jmap_peer_auth::usable_keys`.
- `anonymous_well_known_session` takes the server's Session object, empties
  `accounts` and `primaryAccounts`, sets `username` to `""`, and adds the
  federation fields. It refuses an empty key list or a key with `d`.
- No async. `#[forbid(unsafe_code)]` at the crate root.

## Gotchas

- `serverUrl` is not checked to be an HTTPS URL. Validate it before you
  deliver to it or probe `/.well-known/jmap` with it.
- `directAddress` is peer-supplied and untrusted. Never use it as a blob
  `fetchUrl` target, and always fall back to the mailbox path.
- Deserializing a `Peer/*` request checks only its shape. The receiver
  still checks that exactly one `Peer/deliver` payload is present, that the
  fields a `Peer/groupUpdate` `action` requires are present, and that the
  caller-identity field matches the authenticated peer.
- `PeerAttachment::fetch_url` is peer-supplied: apply the §Blob Fetch and
  SSRF restrictions before fetching it.
- `FederationSession` does not check that `peerSigningKeys` entries are
  usable keys; the receiver must ignore the ones that are not (§Key
  Publication).

## References

- [draft-atwood-jmap-chat-federation-00] — JMAP Chat federation
- [draft-atwood-jmap-chat-00] — JMAP Chat

[draft-atwood-jmap-chat-federation-00]: https://github.com/MarkAtwood/jmap-chat-spec
[draft-atwood-jmap-chat-00]: https://github.com/MarkAtwood/jmap-chat-spec
