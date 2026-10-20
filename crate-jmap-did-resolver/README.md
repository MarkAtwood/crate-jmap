# jmap-did-resolver

DID resolution I/O and caching for the `jmap-*` crate family: `did:key`,
`did:web` and `did:plc`, the three methods draft-atwood-jmap-chat-did-00
requires.

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
// If a signature fails to verify against `resolved`:
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
- Fetched results are cached for a fixed TTL, up to a fixed number of
  DIDs (default 10 000; the oldest goes first). Errors are never cached,
  and an expired entry is never served when its refetch fails.
- `refresh` drops the cached entry, then fetches. Call it when a
  signature fails against a cached document ("A server MUST refresh its
  cached DID Document ... before next use if a signature verification
  against that cached document fails").
- `HttpFetcher::new` is HTTPS-only, times out after 10 s and caps bodies
  at 1 MiB. Implement `Fetch` to use another HTTP stack.

## Gotchas

- The `didDocumentUri` hint of the account capability is not used.
- `did:plc` returns `PlcState`, not a DID document.
- No rate limit on `refresh`: a peer that keeps sending bad signatures
  makes one fetch per attempt. Rate-limit per peer in front of it.

## References

- [draft-atwood-jmap-chat-did-00] — JMAP Chat DID binding (§Caching,
  §Resolution failure handling)
- [draft-atwood-jmap-did-00] — JMAP Decentralized Identifiers
- [did:web] — did:web Method Specification
- [did:key] — The did:key Method v0.9
- [did:plc] — did:plc Method Specification v0.3.0

[draft-atwood-jmap-chat-did-00]: https://github.com/MarkAtwood/jmap-chat-spec
[draft-atwood-jmap-did-00]: https://github.com/MarkAtwood/jmap-chat-spec
[did:web]: https://w3c-ccg.github.io/did-method-web/
[did:key]: https://w3c-ccg.github.io/did-key-spec/
[did:plc]: https://web.plc.directory/spec/v0.1/did-plc
