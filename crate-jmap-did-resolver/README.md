# jmap-did-resolver

DID resolution I/O and caching for the `jmap-*` crate family: `did:key`,
`did:web` and `did:plc`, the three methods draft-atwood-jmap-chat-did-00
requires, and the `did:webvh` it recommends.

## What it is

A support crate. It fetches what the pure helpers in `jmap-did-types`
check, and caches the result. No `*-types` or `*-server` crate depends on
it; servers and clients that need to resolve DIDs opt in.

## How to use

```toml
[dependencies]
jmap-did-resolver = "0.3"
```

```rust,no_run
use std::time::Duration;
use jmap_did_resolver::{HttpFetcher, Resolver, jmap_did_types::Did};

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let resolver = Resolver::new(HttpFetcher::new()?, Duration::from_secs(300));
let did: Did = "did:web:example.com".parse()?;
let resolved = resolver.resolve(&did).await?;
// If the signing key is not in `resolved`'s `authentication` keys:
let resolved = resolver.refresh(&did).await?;
# Ok(()) }
```

## How it works

- `did:key`: the document is derived from the DID. No fetch, no cache.
- `did:web`: GET the URL from `Did::web_document_url`, reject a document
  whose `id` is not the DID.
- `did:plc`: GET `<directory>/<did>/log/audit` (default
  `https://plc.directory`) and verify it with `verify_plc_audit_log`. The
  result is the verified `PlcState`; a tombstoned DID is an error.
- `did:webvh`: GET the `did.jsonl` at `webvh_log_url` and verify it with
  `verify_webvh_log`; when an entry needs witnessing, also GET the
  `did-witness.json` beside it. The result is the verified `WebvhLog`:
  use `current()` for a request being received. A log with an invalid
  entry is returned but not cached.
- `resolve_webvh_version` returns the version an object was accepted
  under, by its recorded `versionId`. The newest verified log of each
  `did:webvh` is kept apart from the TTL cache, so a verified version
  stays available after expiry or a failed refresh. A fetched log that
  lacks the latest `versionId` seen before is a `WebvhRollback` error
  (DIF: resolvers "SHOULD warn or fail resolution when presented with a
  truncated log").
- Fetched results are cached for a fixed TTL, up to a fixed number of
  DIDs (default 10 000; the oldest goes first). Errors are never cached,
  and an expired entry is never served when its refetch fails.
- `refresh` drops the cached entry, then fetches. Call it when the key
  that verified a request signature is not in the `authentication`
  relationship of the resolved document, then check once more with the
  result ("the server MUST resolve the DID again, bypassing its cache,
  before failing the verification"). A failed refresh leaves nothing
  cached.
- `refresh` fetches one DID at most once per 60 s
  (`DEFAULT_REFRESH_INTERVAL`; change it with `with_refresh_interval`).
  Within that interval it returns what `resolve` returns ("A server
  SHOULD NOT resolve one DID again for this reason more often than once
  every 60 seconds").
- `HttpFetcher::new` is HTTPS-only, times out after 10 s and caps bodies
  at 1 MiB. Implement `Fetch` to use another HTTP stack.

## Gotchas

- The `didDocumentUri` hint of the account capability is not used.
- `did:plc` returns `PlcState`, not a DID document.
- `did:webvh` returns the whole `WebvhLog`, not a DID document, and its
  `current()` is `None` when an entry is invalid or the DID is
  deactivated.
- The `did:webvh` `ttl` parameter is not used; the resolver's own TTL
  applies. The witness file is always fetched from the method-derived
  URL.
- A `versionId` newer than the cached log is found only after
  `refresh`.

## References

- [draft-atwood-jmap-chat-did-00] — JMAP Chat DID binding (§Caching,
  §Resolution failure handling)
- [draft-atwood-jmap-did-00] — JMAP Decentralized Identifiers
- [did:web] — did:web Method Specification
- [did:key] — The did:key Method v0.9
- [did:plc] — did:plc Method Specification v0.3.0
- [did:webvh] — The did:webvh DID Method v1.0 (DIF)

[draft-atwood-jmap-chat-did-00]: https://github.com/MarkAtwood/jmap-chat-spec
[draft-atwood-jmap-did-00]: https://github.com/MarkAtwood/jmap-chat-spec
[did:web]: https://w3c-ccg.github.io/did-method-web/
[did:key]: https://w3c-ccg.github.io/did-key-spec/
[did:plc]: https://web.plc.directory/spec/v0.1/did-plc
[did:webvh]: https://identity.foundation/didwebvh/v1.0/
