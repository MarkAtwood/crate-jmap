# jmap-peer-auth-io

The I/O half of JMAP Chat federation peer authentication
(draft-atwood-jmap-chat-federation-00 §Peer Authentication Model):
fetching and caching a peer domain's `peerSigningKeys`, the
DNSSEC-validated `_jmap-peer` TXT lookup, and the nonce replay store.

## What it is

A support crate. It supplies what the I/O-free `jmap-peer-auth` crate
takes as input. No `*-types` or `*-server` crate depends on it; servers
that accept federated peer requests opt in.

## How to use

```toml
[dependencies]
jmap-peer-auth-io = "0.3"
```

```rust,no_run
use std::time::SystemTime;
use jmap_peer_auth_io::jmap_peer_auth::{bind_identity, Request};
use jmap_peer_auth_io::{authenticate, HickoryLookup, HttpsFetcher, MemoryReplayStore, PeerKeys};

# async fn run(request: Request<'_>) -> Result<(), Box<dyn std::error::Error>> {
let keys = PeerKeys::new(HttpsFetcher::new()?, HickoryLookup::from_system_conf()?);
let replay = MemoryReplayStore::new();
let peer = authenticate(request, SystemTime::now(), &keys, &replay).await?;
// Then §Identity Binding on peer.owner_user_id with bind_identity.
# Ok(()) }
```

Any `Err` means reject the request (HTTP 403).

## How it works

- `PeerKeys` GETs `https://<peer-domain>/.well-known/jmap` (unsigned) and
  queries TXT `_jmap-peer.<peer-domain>.` together, then applies
  `jmap_peer_auth::usable_keys`. It caches the result for the response's
  HTTP freshness lifetime (`max-age`, or `Expires` minus `Date`, less
  `Age`), at most one hour, and at most the DNS TTL.
- An unknown `kid`, a bogus DNSSEC answer, an expired entry or no keys
  after a failed fetch forces a new fetch and a new DNS query, at most
  once every 60 seconds per peer domain. Within that interval the request
  is rejected, so a peer whose responses are fresh for less than 60
  seconds (`no-store`, or no `Cache-Control`) is usable only on the first
  request of each interval. Keys are never served past their freshness
  lifetime.
- One fetch per peer domain runs at a time; concurrent requests wait for
  it. At most 10 000 peer domains are tracked by default.
- `HttpsFetcher` is HTTPS-only, validates each hop's certificate against
  its host, follows only `https` redirects (at most 10), times out after
  10 s and caps bodies at 1 MiB. `HttpsFetcher::with_policy` takes an
  address predicate for the draft's SSRF rules; it sees every resolved
  address and every IP-literal redirect target. `HttpsFetcher::new`
  allows every address.
- `HickoryLookup` validates DNSSEC in-process from hickory's root trust
  anchor and builds a fresh resolver per query, so no DNS answer is ever
  reused. `NoDnssec` reports every zone as insecure, which the draft
  requires of a server that can neither validate nor reach a trusted
  validating resolver.
- `MemoryReplayStore` records each accepted (peer domain, nonce) until 60
  seconds after the signature's `expires`. Implement `ReplayStore` over a
  shared store when several processes accept requests for one server.

## Gotchas

- `HttpsFetcher::new` applies no SSRF restriction. Pass a policy that
  refuses loopback, link-local and private ranges in production.
- A proxy configured on the reqwest builder resolves names itself and
  bypasses the address policy.
- A response without explicit freshness (`max-age` or `Expires`) is not
  reused, so such a peer is fetched on every request.
- The keys are fetched before the signature is checked, as they must be
  to check it.
- Upstream name servers must pass DNSSEC records through. One that
  strips signatures from a signed zone makes the answer bogus, and the
  request is rejected.

## References

- [draft-atwood-jmap-chat-federation-00] — §Peer Authentication Model,
  §Key Publication, §DNSSEC Key Binding, §Blob Fetch and SSRF
- [RFC 9111] — HTTP Caching (freshness lifetime)
- [RFC 4033] — DNS Security Introduction and Requirements (§5 validation
  results)
- [RFC 2308] — Negative Caching of DNS Queries (§5 negative TTL)

[draft-atwood-jmap-chat-federation-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 9111]: https://www.rfc-editor.org/rfc/rfc9111
[RFC 4033]: https://www.rfc-editor.org/rfc/rfc4033
[RFC 2308]: https://www.rfc-editor.org/rfc/rfc2308
