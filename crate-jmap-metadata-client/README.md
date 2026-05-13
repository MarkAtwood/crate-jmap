# jmap-metadata-client

## What it is

Typed client methods for the JMAP Object Metadata extension
([draft-ietf-jmap-metadata-01]). Depends on [`jmap-base-client`] for
transport, authentication, and session management, and on
[`jmap-metadata-types`] for wire types.

## What it's for

Implements draft-ietf-jmap-metadata-01 method bindings on top of
`jmap-base-client`: `Metadata/get|changes|set|query|queryChanges`. Sibling
of `jmap-mail-client` in the extension-client family — mirrors that crate's
shape. Depends on `jmap-base-client` for transport and session, and on
`jmap-metadata-types` for the wire types.

## How to use

```rust,no_run
use jmap_base_client::{BearerAuth, ClientConfig, JmapClient};
use jmap_metadata_client::{JmapMetadataExt, MetadataChangesParams};
use jmap_types::{Id, State};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Build a base client (handles auth, HTTP, session fetch).
    let auth = BearerAuth::new("my-token")?;
    let client = JmapClient::new_plain(auth, "https://jmap.example.com", ClientConfig::default())?;

    // 2. Fetch the JMAP session document.
    let session = client.fetch_session().await?;

    // 3. Bind the session to a metadata client.
    let metadata = client.with_metadata_session(session);

    // 4. Fetch all Metadata in the account.
    let response = metadata.metadata_get(None, None).await?;
    for m in &response.list {
        println!(
            "{} on {} {}",
            m.type_name(),
            m.related_type(),
            m.related_id(),
        );
    }

    // 5. Fetch only Annotation changes for Email objects.
    let params = MetadataChangesParams {
        filter_related_type: Some("Email".into()),
        filter_metadata_type: Some(vec!["Annotation".into()]),
        extra: serde_json::Map::new(),
    };
    let changes = metadata
        .metadata_changes(&State::from("s1"), None, Some(params))
        .await?;
    println!(
        "{} created / {} updated / {} destroyed",
        changes.created.len(),
        changes.updated.len(),
        changes.destroyed.len(),
    );

    Ok(())
}
```

After constructing a `SessionClient` via `with_metadata_session`, all JMAP
Metadata methods are available without passing `&Session` on every call.
If the session expires, re-fetch with `JmapClient::fetch_session` and
construct a new `SessionClient`.

Id parameters are typed `&jmap_types::Id` (or `&[jmap_types::Id]` for
slices) to make invalid Ids unrepresentable. State tokens use
`&jmap_types::State`. Construct Ids with `Id::new_validated(s)` to
enforce RFC 8620 §1.2 syntax at the boundary, or with `Id::from(s)` when
the value is known-valid (e.g. already came back from a server response).

## Registered methods

All method implementations live on `SessionClient` in
`src/methods/metadata.rs`.

| Method | Function | Returns |
|---|---|---|
| `Metadata/get` | `metadata_get` | `GetResponse<Metadata>` |
| `Metadata/changes` | `metadata_changes` | `ChangesResponse` |
| `Metadata/set` | `metadata_set` | `SetResponse<Metadata>` |
| `Metadata/query` | `metadata_query` | `QueryResponse` |
| `Metadata/queryChanges` | `metadata_query_changes` | `QueryChangesResponse` |

### Metadata/changes — optional filter args

Per draft §3.3, `Metadata/changes` accepts two extra optional arguments:

- `filterRelatedType` — restrict to Metadata whose `relatedType` equals
  the value.
- `filterMetadataType` — restrict to Metadata whose `@type` is in the
  array.

Pass them via `MetadataChangesParams`. Per §3.3 the state token in the
response represents the complete account state and is independent of
the filters; callers MUST re-use the same filter values across
subsequent `Metadata/changes` calls to ensure consistent
synchronisation.

### Metadata/set

`metadata_set` accepts `create`, `update`, and `destroy` per RFC 8620
§5.3. Server-side uniqueness, quota, `maySetPrivate` gating, and
related-object validation are reported back via the standard SetError
catalogue (`alreadyExists`, `forbidden`, `overQuota`,
`invalidProperties`); see draft §3.1 for details.

## How it works

Every method follows the same five-step pattern:

1. Validate arguments (empty-string guards return `InvalidArgument`
   before any network call).
2. Call `session_parts()` to extract `(api_url, account_id)` from the
   bound session.
3. Build the JMAP method arguments as a `serde_json::Value`.
4. Call `build_request(method_name, args, USING_METADATA)` to construct
   a single-method `JmapRequest`.
5. POST to the API URL and extract the typed response via
   `jmap_base_client::extract_response`.

The capability `using` array for all metadata requests is:
`["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:metadata"]`.

## Gotchas

- **Pinned to draft-01.** This crate tracks
  [draft-ietf-jmap-metadata-01]; later draft revisions (or a final RFC)
  may rename methods, alter argument shapes, or change response fields.
- **`Metadata/changes` filter consistency.** Per draft §3.3 the state
  token returned represents the complete account state and is
  independent of `filterRelatedType` / `filterMetadataType`. Callers
  MUST re-use the same filter values across subsequent
  `Metadata/changes` calls; mixing different filters against the same
  state token will produce inconsistent synchronisation results.

## Crate family

```
jmap-types
    └── jmap-base-client          transport, auth, session
            └── jmap-metadata-client  ← this crate
                    (also depends on jmap-metadata-types for response types)
```

## References

- **[draft-ietf-jmap-metadata-01]** — JMAP Object Metadata (normative
  for all method names, argument shapes, and response formats).
- **[RFC 8620]** — JMAP Core (request format, response shapes, `/get`,
  `/set`, `/changes`, `/query`, `/queryChanges`).

[draft-ietf-jmap-metadata-01]: https://www.ietf.org/archive/id/draft-ietf-jmap-metadata-01.txt
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-base-client`]: ../crate-jmap-base-client
[`jmap-metadata-types`]: ../crate-jmap-metadata-types
