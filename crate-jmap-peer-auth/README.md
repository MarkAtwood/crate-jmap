# jmap-peer-auth

Peer authentication for JMAP Chat federation
(draft-atwood-jmap-chat-federation-00 §Peer Authentication Model):
RFC 9421 HTTP Message Signatures with Ed25519 for signing and verifying
peer requests, the `peerSigningKeys` JWK rules, the DNSSEC precedence
rule and the identity-host binding rule.

## What it is

The parts of peer authentication that need no I/O. You pass in the
clock, the peer domain's fetched `peerSigningKeys` and your DNSSEC
validation result. Fetching keys over HTTPS,
the `_jmap-peer` TXT lookup, the key cache, the 60-second refetch limit
and the replay store are yours (or a support crate's). Server crates may
depend on this crate.

## How to use

```toml
[dependencies]
jmap-peer-auth = "0.4"
```

Sending a request to a peer:

```rust,ignore
let out = jmap_peer_auth::sign(&key, &my_domain, &Method::POST, target_uri, &body, SystemTime::now())?;
// Add out.content_digest (when Some), out.signature_input and out.signature
// as Content-Digest, Signature-Input and Signature.
```

Receiving one:

```rust,ignore
let signed = SignedRequest::parse(Request { method, target_uri, headers, content })?;
let keys = usable_keys(&peer_signing_keys_of(signed.peer_domain()), &dnssec_answer)?;
let accepted = signed.verify(&keys, SystemTime::now())?;
// Check 5: one atomic test-and-insert, true when the pair was new.
if !replay_store.insert(&accepted.peer_domain, &accepted.nonce, accepted.forget_after) {
    return Err(Rejection::Replayed);
}
match bind_identity(&owner_user_id, &accepted.peer_domain) {
    IdentityBinding::Bound => { /* grant peer-role access */ }
    IdentityBinding::NeedsDidVerification => { /* draft-atwood-jmap-chat-did-00 check */ }
    _ => { /* reject */ }
}
```

Any `Rejection` is HTTP 403. On `Rejection::UnknownKey`, refetch the
peer's keys (at most once per 60 seconds per peer domain) and verify
again before rejecting.

## How it works

- `sign` covers `@method`, `@target-uri` and, when there is content,
  `content-digest` (`sha-256`). Parameters: `created`, `expires` (created
  + 300 s), a 128-bit base64url `nonce`, `keyid` = `<peer-domain>#<kid>`,
  `alg="ed25519"`, `tag="jmap-chat-peer"`. The label is `peer`.
- `SignedRequest::parse` runs check 1 (exactly one `jmap-chat-peer`
  signature, `alg`, required components) and parses the `keyid`, so you
  know which peer domain's keys to fetch.
- `SignedRequest::verify` runs the clock window (60 s skew, 300 s
  lifetime), the `Content-Digest` match, the key lookup, Ed25519
  verification (strict). It returns the authenticated peer domain and
  the (domain, nonce, forget-after) record for your atomic replay
  test-and-insert.
- `PeerSigningKey::from_jwk` ignores any entry that is not an OKP
  Ed25519 key whose `kid` is its RFC 7638 thumbprint, or that carries
  the private member `d`.
- `usable_keys` applies §DNSSEC Key Binding: a secure answer with
  `v=jmappeer1; kid=<kid>` records restricts the keys to those kids, a
  bogus answer rejects, and every other answer uses the HTTPS keys alone.
- Peer domains compare in lowercase A-label form without a trailing dot
  (`PeerDomain`).
- `fetch_address_allowed` applies §Blob Fetch and SSRF to one address:
  `false` for loopback, unspecified, link-local, RFC 1918 and RFC 4193
  addresses, and IPv4-mapped forms of them. It fits
  `jmap_peer_auth_io::HttpsFetcher::with_policy`.

Structured fields are parsed and serialized with [`sfv`]; signatures use
[`ed25519-dalek`]. The [`httpsig`] crate was not used: it pulls a second
generation of the RustCrypto elliptic-curve crates and uses
`ed25519-compact`, for a profile that needs three covered components.

## Gotchas

- `target_uri` must be the absolute URI the signer used. Behind a
  proxy, rebuild it from the original scheme and `Host`.
- Empty content counts as no content: `content-digest` is then neither
  added nor required.
- Covered components beyond the required ones may be `@scheme`,
  `@authority`, `@path`, `@query` or plain header fields. Others
  (`@request-target`, `@query-param`, parameterized identifiers) are
  rejected.
- A receiver without a trusted DNSSEC-validating resolver passes
  `DnssecAnswer::Insecure`.
- `fetch_address_allowed` judges an address, not a URL: call it on every
  address you connect to, after resolution and on each redirect. It does
  not check the draft's "peer's known IP range or hostname" rule, and
  lets other special-purpose ranges (100.64.0.0/10, NAT64) through.

## References

- draft-atwood-jmap-chat-federation-00 §Peer Authentication Model
- [RFC 9421] — HTTP Message Signatures
- [RFC 9530] — Digest Fields
- [RFC 8037] — CFRG curves in JOSE (OKP keys)
- [RFC 7638] — JWK Thumbprint
- [RFC 8941] — Structured Field Values for HTTP

[RFC 9421]: https://www.rfc-editor.org/rfc/rfc9421
[RFC 9530]: https://www.rfc-editor.org/rfc/rfc9530
[RFC 8037]: https://www.rfc-editor.org/rfc/rfc8037
[RFC 7638]: https://www.rfc-editor.org/rfc/rfc7638
[RFC 8941]: https://www.rfc-editor.org/rfc/rfc8941
[`sfv`]: https://crates.io/crates/sfv
[`ed25519-dalek`]: https://crates.io/crates/ed25519-dalek
[`httpsig`]: https://crates.io/crates/httpsig
