# jmap-platform-push

JMAP push delivery through the vendor push platforms: Google FCM, Apple
APNs, Amazon ADM, Huawei Push Kit, Xiaomi MiPush and Microsoft WNS. It
wraps a JMAP push payload in each platform's envelope, authenticates,
sends it to one device and says whether the subscription is still good.

## What it is

A support crate. Decision 40 (`docs/design/DECISIONS.md`) makes delivery
to platforms other than Web Push the deploying server's job; this crate is
an optional helper for it, as `jmap-web-push` is for Web Push.

| Item | Platform | What it does |
|---|---|---|
| `Message`, `PushKind` | all | the payload (any `Serialize`), urgency, TTL, data key (`"jmap-push"` by default), and `Data` / `Alert { body }` / `Voip` |
| `PlatformPusher` | all | object-safe trait: `push(destination, &Message) -> Outcome` |
| `Outcome` | all | `Delivered(Receipt)`, `Gone` (destroy the subscription), `Retry { after }`, `Failed` |
| `fcm::FcmPusher`, `fcm::ServiceAccount` | FCM HTTP v1 | `data` map with the payload as a JSON string, `android` priority and TTL or an `apns` block (`DeviceFamily::Apple`); OAuth 2.0 JWT-bearer grant with an RS256 service-account key, token cached |
| `apns::ApnsPusher`, `apns::ApnsKey` | APNs | HTTP/2, ES256 provider token (reused 40 min, remade on `ExpiredProviderToken`), background / alert (`mutable-content`) / VoIP (`.voip` topic) pushes, `apns-priority` and `apns-expiration` |
| `adm::AdmPusher` | ADM | client-credentials token, `data` map, `priority`, `expiresAfter`, the `registrationID` update |
| `hpk::HpkPusher` | Huawei Push Kit | client-credentials token, `data` string, `android.urgency` and `ttl`, the `code` result |
| `mipush::MiPusher`, `mipush::invalid_reg_ids` | MiPush | `Authorization: key=`, form-encoded pass-through message, invalid-regId receipts |
| `wns::WnsPusher`, `wns::WnsCredentials` | WNS | `wns/raw` with `application/octet-stream`, legacy (Package SID) or Entra ID token, `X-WNS-Status` |

## What it's for

A JMAP server whose clients register FCM, APNs, ADM, Push Kit, MiPush or
WNS tokens keeps one pusher per platform and app, and calls it for each
`PushSubscription` push the `jmap-server` / `jmap-chat-server` push
builders produce (`StateChange`, `ChatMessagePush`, `VTCCallPush`). No
`*-types` or `*-server` crate depends on it. Web Push subscriptions go to
`jmap-web-push`.

## How to use

```toml
[dependencies]
jmap-platform-push = "0.6"
```

```rust,no_run
use std::time::Duration;
use jmap_platform_push::fcm::{FcmPusher, ServiceAccount};
use jmap_platform_push::{Message, Outcome, PlatformPusher, Urgency};
use jmap_platform_push::jmap_types::StateChange;

# async fn run(key_file: &str, token: &str, change: StateChange)
#     -> Result<(), Box<dyn std::error::Error>> {
let fcm: Box<dyn PlatformPusher> =
    Box::new(FcmPusher::new(ServiceAccount::from_json(key_file)?)?);
let message = Message::new(&change, Duration::from_secs(86400))?
    .with_urgency(Urgency::Normal);
match fcm.push(token, &message).await {
    Outcome::Delivered(_) => {}
    Outcome::Gone(_) => { /* destroy the PushSubscription */ }
    Outcome::Retry { after, .. } => { /* reschedule, no sooner than `after` */ }
    Outcome::Failed(e) => { /* log; fix the payload or configuration */ }
    _ => {}
}
# Ok(()) }
```

## How it works

- Every pusher makes one attempt. When the platform reports the access
  token or provider token expired (FCM, ADM, WNS `401`; Push Kit
  `80200001`/`80200003`; APNs `ExpiredProviderToken`), it drops the cached
  token, gets a new one and resends once. Backoff is the caller's:
  `Outcome::Retry` carries the platform's `Retry-After` in seconds (FCM
  `QUOTA_EXCEEDED` at least 60 s, per the FCM error-code guidance).
- Access tokens are cached per pusher behind a lock, so concurrent pushes
  make one token request, and replaced a tenth of their lifetime (at most
  five minutes) before `expires_in` runs out.
- `Gone` comes only from an explicit platform signal: FCM `UNREGISTERED`
  or `SENDER_ID_MISMATCH`; APNs `BadDeviceToken`, `DeviceTokenNotForTopic`,
  `Unregistered`, `ExpiredToken` or `410`; ADM `InvalidRegistrationId` or
  `Unregistered`; Push Kit `80300007` (or `80100000` listing the token);
  WNS `404` or `410`. Credential and configuration errors (APNs
  `BadTopic`, `InvalidProviderToken`; FCM `THIRD_PARTY_AUTH_ERROR`; a
  refused token request) are `Failed`, never `Gone`.
- Urgency: `high` is the platform's top priority (FCM `HIGH`, ADM `high`,
  Push Kit `HIGH`, APNs alert priority 10); every lower urgency is the
  normal one. `PushKind::Voip` is always top priority. On APNs an
  `Alert` push of urgency `low` or `very-low` goes as a background push,
  as the guide's urgency table says.
- Clients built by `new` follow no redirects (a bearer token never follows
  one) and time out after 10 s; the APNs client speaks HTTP/2 only.
  `from_client` takes any `reqwest::Client`.
- JWTs: RS256 with ring for the FCM service-account assertion, ES256 with
  p256 for the APNs provider token. Tests check both signatures with
  other implementations (`rsa`, ring).
- `Debug` never prints a private key, client secret, App Secret or
  access token.

## Gotchas

- MiPush answers a send with `result: "ok"` whether or not the regId is
  still valid; invalid regIds come in a type 16 message receipt. Set
  `MiPusher::with_callback`, serve that URL, and pass each receipt's
  `data` field to `mipush::invalid_reg_ids`. A failed MiPush send never
  yields `Gone`, and Xiaomi publishes no `code` table for it, so `code`
  is passed through as `PlatformError::reason`.
- An APNs device token belongs to one environment: a sandbox token sent
  to production is `BadDeviceToken`, which destroys the subscription. Keep
  one `ApnsPusher` per environment and route by how the device registered.
- FCM cannot send APNs VoIP pushes; a `Voip` message through an
  `FcmPusher` for `DeviceFamily::Apple` is `Failed`. Use `ApnsPusher`.
- FCM's `token` field is deprecated in favour of `fid`, but still takes
  registration tokens and FIDs; this crate sends `token`.
- FCM `INVALID_ARGUMENT` can also mean a bad token, but only when the
  payload is valid; it is `Failed`, never `Gone`.
- WNS channel URIs must be `https` in `notify.windows.com`; anything else
  is refused before a token is fetched. `danger_accept_any_channel` turns
  the check off for a local test double only.
- Size limits are the platforms': APNs (4096 octets, 5120 for VoIP) and
  WNS (5000) bodies are checked before sending; FCM (4096), ADM (6 KB of
  `data`), Push Kit (4096) and MiPush (4 KB) are left to the platform's
  answer, which is `Failed`. Fit the payload first (for chat,
  `jmap_chat_server::push::fit_chat_message_push`).
- Only the delay-seconds form of `Retry-After` is read; an HTTP-date is
  ignored.
- No SSRF policy, no rate limiting, no scheduling of retries: those are
  the caller's.
- Every platform here can read the payload. Send content-free payloads
  (`StateChange`) where metadata matters (jmap-push-platform-guide
  §Security).

## References

- [RFC 8620] §7.2 — JMAP push
- jmap-push-platform-guide.md, jmap-chat-push-platform-guide.md and
  jmap-vtc-ring-push-guide.md (jmap-chat-spec)
- FCM: [HTTP v1 Message], [ErrorCode], [FCM error codes], [Google service-account OAuth]
- APNs: [Sending notification requests], [Handling responses], [Token-based connection]
- ADM: [ADM access token], [ADM send message]
- Huawei Push Kit: [HPK send API], [HPK AndroidConfig], [Huawei OAuth 2.0]
- MiPush: [MiPush server API], [MiPush server SDK guide] §4.7
- WNS: [WNS headers], [WNS overview], [WNS quickstart]

[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[HTTP v1 Message]: https://firebase.google.com/docs/reference/fcm/rest/v1/projects.messages
[ErrorCode]: https://firebase.google.com/docs/reference/fcm/rest/v1/ErrorCode
[FCM error codes]: https://firebase.google.com/docs/cloud-messaging/error-codes
[Google service-account OAuth]: https://developers.google.com/identity/protocols/oauth2/service-account
[Sending notification requests]: https://developer.apple.com/documentation/usernotifications/sending-notification-requests-to-apns
[Handling responses]: https://developer.apple.com/documentation/usernotifications/handling-notification-responses-from-apns
[Token-based connection]: https://developer.apple.com/documentation/usernotifications/establishing-a-token-based-connection-to-apns
[ADM access token]: https://developer.amazon.com/docs/adm/request-access-token.html
[ADM send message]: https://developer.amazon.com/docs/adm/send-message.html
[HPK send API]: https://developer.huawei.com/consumer/en/doc/HMSCore-References/https-send-api-0000001050986197
[HPK AndroidConfig]: https://developer.huawei.com/consumer/en/doc/HMS-References/push-sendapi
[Huawei OAuth 2.0]: https://developer.huawei.com/consumer/en/doc/HMSCore-Guides/open-platform-oauth-0000001053629189
[MiPush server API]: https://dev.mi.com/xiaomihyperos/documentation/detail?pId=1696
[MiPush server SDK guide]: https://dev.mi.com/xiaomihyperos/documentation/detail?pId=1695
[WNS headers]: https://learn.microsoft.com/en-us/windows/apps/develop/notifications/push-notifications/push-request-response-headers
[WNS overview]: https://learn.microsoft.com/en-us/windows/apps/develop/notifications/push-notifications/wns-overview
[WNS quickstart]: https://learn.microsoft.com/en-us/windows/apps/develop/notifications/push-notifications/push-quickstart
