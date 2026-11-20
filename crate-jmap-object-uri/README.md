# jmap-object-uri

The `jmap` URI scheme of draft-atwood-jmap-object-uri-00: JMAP object URIs
of the form `jmap:<type>:<accountId>:<objectId>`, which name one JMAP
object by type name, account and id. This crate parses, validates and
formats them, holds the "JMAP Object URI Types" registry, and resolves a
URI to the `/get` call that dereferences it in a given Session.

## What it is

| Item | Spec section |
|---|---|
| `JmapUri` (`FromStr`, `Display`, `new`, `data_type`, `account_id`, `object_id`), `JmapUriError` — re-exported from `jmap-types` | §JMAP Object URIs, §Equivalence |
| `SCHEME`, `is_jmap_uri` | §URI Scheme Registration, §Other Forms |
| `URI_TYPES`, `UriTypeEntry`, `uri_type`, `uri_type_for_data_type` | §JMAP Object URI Types Registry, and the registrations of the drafts below |
| `resolve_type`, `resolve`, `Resolution` (`capability`, `get_method`, `get_arguments`), `SessionView`, `ResolveError` | §Interpretation, §Security Considerations |

The registry rows are the type names requested by draft-atwood-jmap-chat-00,
-chat-calendars-00, -chat-tasks-00, -chat-filenode-00, -graph-00,
-graph-projection-00, -scene-00 and -vtc-00. Each maps to a row of the
RFC 9404 "JMAP Data Types" registry in `jmap_types::data_types`, which
gives the capability to put in `using`.

## What it's for

- Clients that receive a JMAP object URI, for example in a JMAP Chat
  MessageAction or Endpoint `uri`, and need the `/get` call that fetches
  the object, or need to know the Session cannot serve it.
- Servers that emit object URIs (Graph projection `objectUri`) and need
  the registered type name for a data type.
- Federation servers that strip every `jmap` URI from peer data
  (draft-atwood-jmap-chat-federation-00 §User References in Peer Methods):
  `is_jmap_uri` is that test.

It depends on `jmap-types` only (for `JmapUri`, `Id` and the Data Types
registry), and not on any client crate: Session access goes through the
`SessionView` trait.

## How to use

```rust
use jmap_object_uri::{JmapUri, ResolveError, resolve};
use serde_json::json;

let uri: JmapUri = "jmap:calendarevent:A13824:E6B2".parse()?;
assert_eq!(uri.to_string(), "jmap:calendarevent:A13824:E6B2");

let session = json!({
    "capabilities": { "urn:ietf:params:jmap:calendars": {} },
    "accounts": { "A13824": { "accountCapabilities": {
        "urn:ietf:params:jmap:calendars": {}
    } } }
});
let found = resolve(&uri, &session)?;
assert_eq!(found.get_method(), "CalendarEvent/get");
assert_eq!(found.capability(), "urn:ietf:params:jmap:calendars");
assert_eq!(found.get_arguments(), json!({ "accountId": "A13824", "ids": ["E6B2"] }));

let task: JmapUri = "jmap:task:A13824:T1".parse()?;
assert_eq!(
    resolve(&task, &session),
    Err(ResolveError::CapabilityNotSupported { capability: "urn:ietf:params:jmap:tasks" })
);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## How it works

`JmapUri::from_str` splits on `:` into at most four parts. `:` is not an
`id-char`, so the split is unambiguous and anything after the object id
stays in it and fails `Id` validation. The scheme matches
case-insensitively, every other part case-sensitively, and `Display`
always writes the lowercase scheme, so parsing then formatting a valid
URI returns the same string apart from the scheme's case.

`resolve_type` looks the type name up in `URI_TYPES` and its data type in
`jmap_types::data_types::DATA_TYPES`. `resolve` then checks the Session's
`capabilities` and the account's `accountCapabilities` for that data
type's capability. `SessionView` is implemented for `serde_json::Value`
(a Session object as JSON); a typed Session implements the two methods.

## Gotchas

- `JmapUri` and `JmapUriError` live in `jmap-types` (other `jmap-*` type
  crates use them) and are re-exported here; they are the same types
  under either path.
- No component is ever percent-encoded (§Equivalence), so `%` anywhere is
  a syntax error; there is no decoding step.
- `jmap://` authority forms are reserved (§Other Forms) and fail to parse
  like any other malformed URI (`InvalidDataType`). `is_jmap_uri` is true
  for them.
- A URI is meaningful only in the Session it arrived with
  (§Interpretation). Nothing in `JmapUri` records that context; keep the
  URI next to the Session or peer it came from, and do not relay it
  unchanged to another context.
- `JmapUri` has no serde impl; wire fields hold the URI as a `String` and
  parse it on use.
- `resolve` succeeding does not mean the object exists: a missing or
  unreadable object shows up in the `/get` `notFound` list.
- The registry is a fixed table of the drafts' registrations; a type name
  registered elsewhere resolves to `UnknownType` until it is added.

## References

- [draft-atwood-jmap-object-uri-00] — JMAP Object URIs
- [draft-atwood-jmap-chat-federation-00] — §User References in Peer Methods
- [RFC 8620] — JMAP Core (§1.2 Id, §2 Session, §5.1 /get)
- [RFC 9404] — JMAP Blob Management (§6.3 JMAP Data Types registry)
- [RFC 3986] — URI Generic Syntax (§3.1 scheme)

[draft-atwood-jmap-object-uri-00]: https://github.com/MarkAtwood/jmap-chat-spec/blob/main/draft-atwood-jmap-object-uri-00.md
[draft-atwood-jmap-chat-federation-00]: https://github.com/MarkAtwood/jmap-chat-spec/blob/main/draft-atwood-jmap-chat-federation-00.md
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[RFC 9404]: https://www.rfc-editor.org/rfc/rfc9404
[RFC 3986]: https://www.rfc-editor.org/rfc/rfc3986
