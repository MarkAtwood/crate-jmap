# jmap-chat-federation-types

Wire-format types for draft-atwood-jmap-chat-federation-00, server-to-server
delivery between JMAP Chat mailboxes. Types only: no I/O, no transport.

## What it is

| Item | Spec |
|---|---|
| `contact_server_url` | §ChatContact Type Extensions for Federation (`serverUrl`) |
| `contact_direct_address` | §ChatContact Type Extensions for Federation (`directAddress`), §Direct Address Hints |

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
- No async. `#[forbid(unsafe_code)]` at the crate root.

## Gotchas

- `serverUrl` is not checked to be an HTTPS URL. Validate it before you
  deliver to it or probe `/.well-known/jmap` with it.
- `directAddress` is peer-supplied and untrusted. Never use it as a blob
  `fetchUrl` target, and always fall back to the mailbox path.

## References

- [draft-atwood-jmap-chat-federation-00] — JMAP Chat federation
- [draft-atwood-jmap-chat-00] — JMAP Chat

[draft-atwood-jmap-chat-federation-00]: https://github.com/MarkAtwood/jmap-chat-spec
[draft-atwood-jmap-chat-00]: https://github.com/MarkAtwood/jmap-chat-spec
