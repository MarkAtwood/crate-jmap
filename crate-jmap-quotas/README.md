# jmap-quotas

JMAP for Quotas ([RFC 9425]) in one dependency: re-exports
[`jmap-quotas-types`] always, [`jmap-quotas-client`] with the `client`
feature and [`jmap-quotas-server`] with the `server` feature (`memory`
adds the server's in-memory reference backend).

```toml
[dependencies]
jmap-quotas = { version = "0.5", features = ["client"] }
```

| Feature | Re-export |
|---|---|
| (none) | `jmap_quotas::types` |
| `client` | `jmap_quotas::client` |
| `server` | `jmap_quotas::server` |
| `memory` | `server`, plus `jmap_quotas::server::memory` |

[RFC 9425]: https://www.rfc-editor.org/rfc/rfc9425
[`jmap-quotas-types`]: https://crates.io/crates/jmap-quotas-types
[`jmap-quotas-client`]: https://crates.io/crates/jmap-quotas-client
[`jmap-quotas-server`]: https://crates.io/crates/jmap-quotas-server
