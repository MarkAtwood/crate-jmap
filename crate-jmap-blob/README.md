# jmap-blob

JMAP Blob Management ([draft-ietf-jmap-blobext-03], `urn:ietf:params:jmap:blob2`,
with legacy [RFC 9404] `urn:ietf:params:jmap:blob` support) in one dependency:
re-exports [`jmap-blob-types`] always, [`jmap-blob-client`] with the `client`
feature and [`jmap-blob-server`] with the `server` feature (`memory` adds the
server's in-memory reference backend).

```toml
[dependencies]
jmap-blob = { version = "0.6", features = ["client"] }
```

| Feature | Re-export |
|---|---|
| (none) | `jmap_blob::types`, `jmap_blob::CAPABILITY` (the RFC 9404 URI, kept from 0.0.1) |
| `client` | `jmap_blob::client` |
| `server` | `jmap_blob::server` |
| `memory` | `server`, plus `jmap_blob::server::memory` |

[draft-ietf-jmap-blobext-03]: https://datatracker.ietf.org/doc/draft-ietf-jmap-blobext/
[RFC 9404]: https://www.rfc-editor.org/rfc/rfc9404
[`jmap-blob-types`]: https://crates.io/crates/jmap-blob-types
[`jmap-blob-client`]: https://crates.io/crates/jmap-blob-client
[`jmap-blob-server`]: https://crates.io/crates/jmap-blob-server
