# jmap-sharing-client

## What it is

Typed client methods for JMAP Sharing ([RFC 9670]). Depends on
[`jmap-base-client`] for transport, authentication, and session management.

## What it's for

Implements RFC 9670 / draft-ietf-jmap-sharing method bindings on top of
`jmap-base-client`: `Principal/get|changes|set|query|queryChanges` and
`ShareNotification/get|changes|set|query|queryChanges`. Sibling of
`jmap-mail-client` in the extension-client family — mirrors that crate's
shape. Depends on `jmap-base-client` for transport and session, and on
`jmap-sharing-types` for the wire types.

## How to use

```rust,no_run
use jmap_base_client::{BearerAuth, ClientConfig, JmapClient};
use jmap_sharing_client::JmapSharingExt;
use jmap_types::Id;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Build a base client (handles auth, HTTP, session fetch).
    let auth = BearerAuth::new("my-token")?;
    let client = JmapClient::new_plain(auth, "https://jmap.example.com", ClientConfig::default())?;

    // 2. Fetch the JMAP session document.
    let session = client.fetch_session().await?;

    // 3. Bind the session to a sharing client.
    let sharing = client.with_sharing_session(session);

    // 4. Fetch all principals.
    let response = sharing.principal_get(None, None).await?;
    for p in &response.list {
        println!("{}: {} ({})", p.id, p.name, p.email.as_deref().unwrap_or("no email"));
    }

    // 5. Query principals by name.
    let filter = serde_json::json!({ "name": "Alice" });
    let query = sharing.principal_query(Some(filter), None, None, None).await?;
    println!("found {} principal(s)", query.ids.len());

    // 6. Fetch share notifications.
    let notifications = sharing.share_notification_get(None, None).await?;
    for n in &notifications.list {
        println!("notification: {} on {} ({})", n.id, n.object_type, n.name);
    }

    // 7. Dismiss a share notification (destroy-only — no create or update).
    sharing.share_notification_set(Some(vec![Id::from("notif-id-1")])).await?;

    Ok(())
}
```

After constructing a `SessionClient` via `with_sharing_session`, all JMAP
Sharing methods are available without passing `&Session` on every call. If the
session expires, re-fetch with `JmapClient::fetch_session` and construct a new
`SessionClient`.

Id parameters are typed `&jmap_types::Id` (or `&[jmap_types::Id]` for slices)
to make invalid Ids unrepresentable. State tokens use `&jmap_types::State`.
Construct Ids with `Id::new_validated(s)` to enforce RFC 8620 §1.2 syntax at
the boundary, or with `Id::from(s)` when the value is known-valid (e.g.
already came back from a server response).

## Registered methods

All method implementations live on `SessionClient` in the `methods/` submodules.

| Method | Function | Returns |
|---|---|---|
| `Principal/get` | `principal_get` | `GetResponse<Principal>` |
| `Principal/changes` | `principal_changes` | `ChangesResponse` |
| `Principal/set` | `principal_set` | `SetResponse<Principal>` |
| `Principal/query` | `principal_query` | `QueryResponse` |
| `Principal/queryChanges` | `principal_query_changes` | `QueryChangesResponse` |
| `ShareNotification/get` | `share_notification_get` | `GetResponse<ShareNotification>` |
| `ShareNotification/changes` | `share_notification_changes` | `ChangesResponse` |
| `ShareNotification/set` | `share_notification_set` | `SetResponse` |
| `ShareNotification/query` | `share_notification_query` | `QueryResponse` |
| `ShareNotification/queryChanges` | `share_notification_query_changes` | `QueryChangesResponse` |

### ShareNotification/set — destroy-only

`share_notification_set` accepts only a `destroy` list. No `create` or `update`
parameter is exposed, because RFC 9670 §3.3 specifies that the server rejects
create and update operations with `forbidden`. This prevents constructing
requests that the server will always reject.

```rust
use jmap_types::Id;

// Dismiss one notification:
sharing.share_notification_set(Some(vec![Id::from("notif-id-1")])).await?;

// Send an empty destroy (no-op, still a valid /set call):
sharing.share_notification_set(None).await?;
```

### Principal/set — permission note

`principal_set` accepts `create`, `update`, and `destroy`. However, RFC 9670
§2.3 specifies that the server may reject any of these with `forbidden` if the
caller lacks sufficient permission. In practice, most servers restrict create
and destroy to administrators; ordinary users can typically only update
`name`, `description`, and `timeZone` on their own Principal.

## How it works

Every method follows the same five-step pattern:

1. Validate arguments (empty-string guards return `InvalidArgument` before any
   network call).
2. Call `session_parts()` to extract `(api_url, account_id)` from the bound
   session.
3. Build the JMAP method arguments as a `serde_json::Value`.
4. Call `build_request(method_name, args, USING_SHARING)` to construct a
   single-method `JmapRequest`.
5. POST to the API URL and extract the typed response via
   `jmap_base_client::extract_response`.

The capability `using` array for all sharing requests is:
`["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:principals"]`.

## Gotchas

`Principal.accounts` is `Option<HashMap<Id, serde_json::Value>>` because
Account capability objects vary per JMAP extension — each extension defines its
own `accountCapabilities` schema. Callers needing typed access to capability
values within an Account should deserialize the `Value` against their own
struct.

## Crate family

```
jmap-types
    └── jmap-base-client          transport, auth, session
            └── jmap-sharing-client  ← this crate
                    (also depends on jmap-sharing-types for response types)
```

## References

- **[RFC 9670]** — JMAP Sharing (normative for all method names, argument
  shapes, response formats, and destroy-only semantics for ShareNotification)
- **[RFC 8620]** — JMAP Core (request format, response shapes, `/get`, `/set`,
  `/changes`, `/query`, `/queryChanges`)

[RFC 9670]: https://www.rfc-editor.org/rfc/rfc9670
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-base-client`]: ../crate-jmap-base-client
