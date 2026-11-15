# jmap-quotas-client

Typed JMAP Quotas ([RFC 9425]) calls over `jmap-base-client`: `Quota/get`,
`Quota/changes`, `Quota/query` and `Quota/queryChanges`.

## How to use

```rust,no_run
# async fn demo(client: jmap_base_client::JmapClient, session: jmap_base_client::Session)
# -> Result<(), jmap_base_client::ClientError> {
use jmap_quotas_client::JmapQuotasExt;

let quotas = client.with_quotas_session(session);
let all = quotas.quota_get(None).await?;
// Later: only usage changed? Fetch just `used`.
let changes = quotas.quota_changes(&all.state, None).await?;
if changes.updated_properties == Some(Some(vec!["used".to_owned()])) {
    let used = quotas.quota_get_properties(Some(&changes.updated), &["used"]).await?;
}
# Ok(()) }
```

## How it works

RFC 9425 §4.1 has the server keep in a Quota's `types` only the types
whose capability the request's `using` lists, and leave out a Quota with
none. The client therefore sends `core`, `quota` and, by default, every
other capability the Session advertises, so every Quota is visible.
`SessionClient::with_type_capabilities` sends only the capabilities you
name instead, for a client that handles only some data types.

`quota_get` decodes full `Quota` objects; `quota_get_properties` returns
the requested properties as JSON objects, since a partial Quota does not
decode as `Quota`.

## References

- **[RFC 9425]** — JMAP for Quotas
- **[RFC 8620]** — JMAP Core

[RFC 9425]: https://www.rfc-editor.org/rfc/rfc9425
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
