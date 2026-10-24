# jmap-web-push

Web Push delivery for the `jmap-*` crate family: RFC 8291 message
encryption, RFC 8292 VAPID, the RFC 8030 `TTL` / `Urgency` / `Topic`
headers, and retry with backoff, for the pushes of RFC 8620 §7.2.

## What it is

A support crate. It POSTs push payloads (`StateChange`,
`PushVerification`, `ChatMessagePush`) built elsewhere to a
`PushSubscription.url`. No `*-types` or `*-server` crate depends on it;
servers that deliver Web Push opt in.

## How to use

```toml
[dependencies]
jmap-web-push = "0.4"
```

```rust,no_run
use std::time::Duration;
use jmap_web_push::{Endpoint, Outcome, PushOptions, Sender, Vapid};
use jmap_web_push::jmap_types::{PushSubscription, StateChange};
use jmap_web_push::p256::ecdsa::SigningKey;

# async fn run(key: SigningKey, sub: PushSubscription, change: StateChange)
#     -> Result<(), Box<dyn std::error::Error>> {
let sender = Sender::new(Vapid::new(key, "mailto:ops@example.com"))?;
let endpoint = Endpoint::try_from(&sub)?;
match sender.send(&endpoint, &change, &PushOptions::new(Duration::from_secs(86400))).await? {
    Outcome::Accepted => {}
    Outcome::Gone => { /* destroy the PushSubscription */ }
}
# Ok(()) }
```

## How it works

- With `keys`, the JSON payload is encrypted as one `aes128gcm` record
  (`rs` 4096) to the subscription's `p256dh` and `auth`, with a fresh salt
  and sender key pair per push. Without `keys` it is sent as is. Either
  way `Content-Type` is `application/json` (RFC 8620 §7.2).
- Every attempt carries a fresh `Authorization: vapid t=..., k=...`: an
  ES256 JWT with `aud` set to the push URL's origin, `sub` set to the
  contact URI, and `exp` 12 hours out.
- `2xx` is `Outcome::Accepted`; `404` and `410` are `Outcome::Gone`.
  `429`, `5xx` and transport errors are retried with exponential backoff
  (default 3 attempts, 1 s base, 60 s cap), waiting out a `Retry-After`
  in seconds. A `Retry-After` past the cap ends the send at once with the
  delay in the error. Any other status is `SendError::Rejected`.
- `Sender::new` follows no redirects and times out after 10 s.
- Payloads over 3993 octets of JSON are refused before sending (RFC 8291
  §4).

Encryption is a thin layer over [`ece-native`] (RFC 8188, RustCrypto).
The [`web-push`] crate was not used: it encrypts through OpenSSL. The
[`web-push-native`] crate was not used: its VAPID `aud` drops the port of
the push URL, and its encryption cannot be checked against the RFC 8291
example. It is the test suite's independent decryptor.

## Gotchas

- No SSRF check on the push URL and no `https://` check: check the URL
  before you build the `Endpoint`.
- No per-subscription rate limit. RFC 8620 §7.2: a 429 "MUST cause the
  JMAP server to reduce the frequency of pushes"; coalesce in front of
  the sender.
- A `403` is `Rejected`, not `Gone`: it usually means the push service
  refused the VAPID token (RFC 8292 §2).
- An HTTP-date `Retry-After` is ignored in favour of the backoff.
- Use a VAPID signing key for nothing else (RFC 8292 §3.2).

## References

- [RFC 8620] §7.2 — JMAP push
- [RFC 8030] — Generic Event Delivery Using HTTP Push
- [RFC 8291] — Message Encryption for Web Push
- [RFC 8292] — VAPID for Web Push
- [RFC 8188] — Encrypted Content-Encoding for HTTP

[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[RFC 8030]: https://www.rfc-editor.org/rfc/rfc8030
[RFC 8291]: https://www.rfc-editor.org/rfc/rfc8291
[RFC 8292]: https://www.rfc-editor.org/rfc/rfc8292
[RFC 8188]: https://www.rfc-editor.org/rfc/rfc8188
[`ece-native`]: https://crates.io/crates/ece-native
[`web-push`]: https://crates.io/crates/web-push
[`web-push-native`]: https://crates.io/crates/web-push-native
