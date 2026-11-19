# JMAP-x3lu push guides MUST audit (jmap-chat-spec 120db2c)

Paths: WP = crate-jmap-web-push/src/lib.rs, WPT = crate-jmap-web-push/tests/web_push.rs,
CP = crate-jmap-chat-server/src/push.rs, CPT = crate-jmap-chat-server/tests/.

JMAP-gxxm (closed, Web Push support crate) does track part of jmap-push-platform-guide: the
Web Push section and the generic lifecycle rules as applied to Web Push (13 MUSTs). It does not
cover any FCM/ADM/HPK/MiPush/APNs/WNS MUST: the workspace has no code for those platforms
(grep FCM/APNs/WNS/MiPush/UNREGISTERED/BadDeviceToken over *.rs: no hits), and no bead mentions
them. JMAP-vjm0 (support-crate decision) names only Web Push. Proposed GAP beads G1-G4 below need
a scope decision first: platform adapters hold vendor secrets and need out-of-band token
registration, so they could go to the consumer. If the decision is "consumer", re-mark every
G row OUT OF SCOPE (owner: the deploying server/consumer).

## Proposed GAP beads

- G1. Support crate: FCM v1 data-message delivery for JMAP push payloads. New support crate (e.g. crate-jmap-platform-push, fcm module). Size M.
- G2. Support crate: APNs delivery for JMAP push (background/alert, VoIP topic, reason parsing). Same crate, apns module. Size M.
- G3. Support crate: ADM, HPK and MiPush delivery. Same crate. Size M.
- G4. WNS raw-notification delivery to `PushSubscription.url` channel URIs (OAuth, wns/raw, X-WNS-* status). crate-jmap-web-push or the same platform crate. Size M. WNS URLs fit `PushSubscription.url` natively, so a WNS channel URI can be registered today and the Web Push `Sender` would POST VAPID+`application/json` to it with no `X-WNS-Type` header.

## jmap-push-platform-guide.md (51)

| doc:line | MUST text (short) | verdict | evidence / bead / reason |
|---|---|---|---|
| push:13 | RFC 2119 keyword note | OUT OF SCOPE | Meta text about keyword use, not a requirement. |
| push:61 | "Servers MUST use data messages, not display messages, except for an APNs alert push that sets "mutable-content": 1" | GAP (G1-G4) | No platform envelopes in kit. Web Push has no display mode, so it is N/A there. |
| push:80 | "Servers MUST NOT destroy the `PushSubscription` in this case" (credential failure) | COVERED | WP:605-609: only 404/410 give `Outcome::Gone`; 401/403 give `SendError::Rejected`; VAPID errors give `SendError::Vapid`. WPT:261 `client_errors_are_rejected_without_retry` (403). JMAP-gxxm. 401 is not tested. |
| push:82 | "Servers MUST only destroy the subscription when the platform explicitly tells the server the token or endpoint is gone" | COVERED (Web Push) | WP:605-606; WPT:167 `not_found_and_gone_mean_gone`. The caller destroys the subscription on Gone (crate-jmap-server/src/push.rs:248 `destroy_push_subscription`). FCM/APNs signals are in G1/G2. |
| push:127 | "`data` values MUST be strings" (FCM) | GAP (G1) | No FCM code. |
| push:129 | "The `"notification"` key MUST be omitted" (FCM) | GAP (G1) | No FCM code. |
| push:131 | "servers MUST replace the `"android"` block with an `"apns"` block" (iOS via FCM) | GAP (G1) | No FCM code. |
| push:145 | "servers MUST map `"low"` and `"very-low"` to `"NORMAL"`" (FCM) | GAP (G1) | No FCM urgency mapping. |
| push:149 | "servers MUST always check the response body" (FCM 200) | GAP (G1) | No FCM code. |
| push:152 | ""UNREGISTERED" or "SENDER_ID_MISMATCH" — servers MUST destroy the `PushSubscription`" | GAP (G1) | No FCM code. |
| push:156 | "INVALID_ARGUMENT ... MUST NOT retry" | GAP (G1) | No FCM code. |
| push:198 | "`data` values MUST be strings" (ADM) | GAP (G3) | No ADM code. |
| push:214 | "Servers MUST parse it [`reason`] — HTTP status alone is not sufficient" (ADM) | GAP (G3) | No ADM code. |
| push:217 | "`InvalidRegistrationId` / `Unregistered` — servers MUST destroy the `PushSubscription`" | GAP (G3) | No ADM code. |
| push:261 | "servers MUST serialize the entire payload JSON into it" (HPK data) | GAP (G3) | No HPK code. |
| push:262 | "The `"notification"` key MUST be omitted" (HPK) | GAP (G3) | No HPK code. |
| push:274 | "Servers MUST map `"low"` and `"very-low"` to `"NORMAL"`" (HPK) | GAP (G3) | No HPK code. |
| push:279 | "Servers MUST parse the `code` field to classify errors" (HPK) | GAP (G3) | No HPK code. |
| push:294 | "Servers MUST include it on every request" (MiPush App Secret) | GAP (G3) | No MiPush code. |
| push:311 | "servers MUST URL-encode the JSON payload and place it there" (MiPush) | GAP (G3) | No MiPush code. |
| push:327 | "Invalid or unregistered regIds indicate the subscription MUST be destroyed" | GAP (G3) | No MiPush code. |
| push:420 | "servers MUST parse `reason`" (APNs) | GAP (G2) | No APNs code. |
| push:422 | "`BadDeviceToken` / `DeviceTokenNotForTopic` — servers MUST destroy the `PushSubscription`" | GAP (G2) | No APNs code. |
| push:424 | "`Unregistered` / `ExpiredToken` — servers MUST destroy the `PushSubscription`" | GAP (G2) | No APNs code. |
| push:427 | "`BadTopic` ... Servers MUST NOT destroy the `PushSubscription` in this case" | GAP (G2) | No APNs code. |
| push:447 | "Clients MUST store all three" (endpoint, p256dh, auth) | OUT OF SCOPE | Client app (browser/UnifiedPush client) behaviour. The server side parses both keys in WP:149-163. |
| push:451 | "Servers MUST generate a P-256 ECDSA key pair once at setup and persist the private key" | OUT OF SCOPE | Deployment/ops: `Vapid::new` (WP:273) takes a caller-supplied `SigningKey`; generating and persisting it belongs to the deploying server operator. |
| push:452 | "Servers MUST include the VAPID header on every request" | COVERED | WP:577-585: a fresh `Authorization: vapid` on every attempt, retries included. WPT:112-116; tests/vapid.rs:53-110 check token shape. JMAP-gxxm. |
| push:464 | "Servers MUST encrypt payload bytes per RFC 8291 (`aes128gcm`)" | COVERED | WP:197-239, 567-568, 589-591; lib.rs:656 `rfc8291_example_ciphertext` (exact RFC 8291 §5 vector); WPT:88 decrypt round trip. JMAP-gxxm. |
| push:486 | "Servers MUST pass them through directly" (urgency) | COVERED | WP:592-594 uses `jmap_types::Urgency::as_str` (crate-jmap-types/src/push.rs:46-48, includes "very-low"); WPT:110. |
| push:500 | "Servers MUST keep plaintext under **3993 bytes**" | COVERED | WP:62, 198-202, 569-571; lib.rs:683 `encrypt_caps_plaintext_at_3993_octets`, WPT:322 `oversized_payload_is_refused_before_sending`. Note: exactly 3993 is allowed, which matches RFC 8291 "at most" rather than the guide's "under". |
| push:505 | "servers MUST treat it [ntfy] identically to any other Web Push delivery" | COVERED | WP:560-629: `Sender::send` has no host-specific branch. Holds by construction; there is no ntfy-specific test. |
| push:509 | "Servers MUST treat any UnifiedPush endpoint as a Web Push endpoint" | COVERED | Same as push:505. |
| push:513 | "a server MUST NOT fall back to unencrypted delivery on that subscription" (keys present) | COVERED | WP:157-161: malformed keys are an `EndpointError`, never dropped. WP:567-568: keys always mean encrypt. No path strips keys. There is no explicit test for the bad-keys case. |
| push:514 | "Operators MUST be aware that unencrypted delivery exposes payload content" | OUT OF SCOPE | Operator awareness (deployment/ops). |
| push:526 | "Clients MUST renew and update `PushSubscription.url` before expiry" (WNS) | OUT OF SCOPE | Windows client app. |
| push:554 | "Servers MUST use `X-WNS-Type: wns/raw` with `Content-Type: application/octet-stream`" | GAP (G4) | WP:586 always sends `application/json` and has no X-WNS-Type. |
| push:557 | "servers MUST NOT use `wns/toast` for JMAP push" | GAP (G4) | Holds vacuously now (no WNS code). It goes in the G4 oracle. |
| push:566 | "Servers MUST check on every response" (X-WNS-NotificationStatus) | GAP (G4) | No WNS header handling: a `200` with `dropped` reads as `Accepted` (WP:602-603). |
| push:576 | "`404 Not Found` / `410 Gone` — channel URI invalid; servers MUST destroy the `PushSubscription`" | GAP (G4) | WP:605 would return Gone for WNS too, but WNS delivery itself is unsupported (G4). |
| push:586 | "For FCM, a `200` does not guarantee delivery — servers MUST always check the response body" | GAP (G1) | No FCM code. |
| push:591 | "For FCM: server MUST check `error.status` in body" | GAP (G1) | No FCM code. |
| push:593 | "400 ... MUST NOT retry this payload" | COVERED | WP:608-609; WPT:261 (400 is sent once). |
| push:594 | "401 ... Server MUST refresh credential and retry" | GAP (G1-G4) | OAuth refresh is only needed on the platforms in G1-G4. Web Push: the VAPID JWT is minted fresh per attempt (WP:577-585), but a 401 is not retried (WP:608-609 -> `Rejected`). Add a 401 retry to the G4/web-push oracle if wanted. |
| push:595 | "403 ... Server MUST parse the body; MUST NOT destroy the `PushSubscription` unless the body says the token is gone" | COVERED (Web Push) / GAP (G1,G2) | Web Push: 403 -> `Rejected`, never `Gone` (WP:608-609; WPT:261). Body parsing for FCM/APNs is G1/G2. |
| push:596 | "404 ... Server MUST destroy the `PushSubscription`" | COVERED | WP:605-606 `Outcome::Gone`; WPT:167. The caller destroys via crate-jmap-server/src/push.rs:248. JMAP-gxxm. |
| push:597 | "410 ... Server MUST destroy the `PushSubscription`" | COVERED | Same as push:596. |
| push:603 | "clients MUST handle it identically to an unaugmented `StateChange`" | OUT OF SCOPE | Client app behaviour (Message/changes on wake). |
| push:628 | "OAuth tokens ... MUST NOT be reused past their validity window" | GAP (G1,G3,G4) | No OAuth platform code. The VAPID analogue is covered: exp is now+12h and minted per attempt (WP:73, 577-581). |
| push:629 | "Static keys ... MUST be stored as server-side secrets — MUST NOT be logged and MUST NOT be embedded in client code" | OUT OF SCOPE | Deployment/ops secret handling (MiPush key, which the kit does not hold). Related: `Vapid` Debug never prints the private key (WP:333-340). |
| push:631 | "implementations MUST treat them [.p8 keys] as long-lived secrets" | OUT OF SCOPE | Deployment/ops key custody. |

## jmap-chat-push-platform-guide.md (15)

| doc:line | MUST text (short) | verdict | evidence / bead / reason |
|---|---|---|---|
| chat:14 | RFC 2119 keyword note | OUT OF SCOPE | Meta text. |
| chat:50 | "Clients MUST NOT request an `urgency` or `mentionUrgency` value not in `supportedUrgencyValues`" | OUT OF SCOPE | Client MUST. The matching server rejection is covered: CP:97-139 `validate_chat_push`; CPT chat_push_validation.rs:102, :114. |
| chat:55 | "Clients MUST fall back to a standard RFC 8620 `PushSubscription` without a `chatPush` property" | OUT OF SCOPE | Client MUST. |
| chat:59 | "Clients MUST NOT include a `chatPush` property ... when the capability is absent" | OUT OF SCOPE | Client MUST. The server rejection is covered: CP:78-86; CPT chat_push_validation.rs:82 `chat_push_for_account_without_capability_is_rejected`. |
| chat:79 | "Clients receiving a `StateChange` on a subscription with `chatPush` configured MUST handle it the same way" | OUT OF SCOPE | Client MUST. |
| chat:100 | "`data` map values MUST be strings. Servers MUST serialize `ChatMessagePush` as JSON and place it under an application-defined key" (FCM/ADM) | GAP (G1,G3) | No FCM/ADM envelope. |
| chat:111 | "Servers MUST serialize `ChatMessagePush` as JSON and place the string there directly" (HPK) | GAP (G3) | No HPK envelope. |
| chat:120 | "Servers MUST URL-encode the `ChatMessagePush` JSON and pass it as the `payload` form field" (MiPush) | GAP (G3) | No MiPush envelope. |
| chat:129 | "Servers MUST embed `ChatMessagePush` inline as a JSON object under an application-defined key" (APNs) | GAP (G2) | No APNs envelope. |
| chat:185 | "Servers MUST derive the platform urgency from `ChatPushConfig.urgency` using the mapping tables" | COVERED (Web Push) | CP:395-401 derives `BuiltPush.urgency`; the Web Push mapping is identity (WP:592-594). CPT chat_push_payload.rs:584 `urgency_selection`. FCM/HPK/APNs mappings are G1-G3. |
| chat:189 | "the server MUST use `mentionUrgency` instead of `urgency` for the entire push" | COVERED | CP:335-337 (`mentioned` = direct or a targeting broadcast scope), CP:395-399; CPT chat_push_payload.rs:584-616 (direct, broadcast, encrypted, mention-only cases). |
| chat:191 | "all messages in that payload MUST share the elevated urgency" | COVERED | By construction: one entry per payload (CP:402-408) and one urgency per `BuiltPush`. There is no batch test because batching is not implemented. |
| chat:192 | "the mention notification MUST NOT be silently demoted" | COVERED | Same as chat:189 and chat:191. |
| chat:249 | "Clients MUST silently ignore unknown scope values in `mentionScopes`" | COVERED (type system) | `ChatMessageEntry.mention_scopes: Vec<BroadcastScope>` (crate-jmap-chat-types/src/push.rs:149). `BroadcastScope::Other(String)` (crate-jmap-chat-types/src/message.rs:253) makes an unknown value deserialize instead of fail. Ignoring it in UI is the client app's job. |
| chat:316 | "Clients MUST NOT use it [senderDisplayName] as an authoritative identity signal" | OUT OF SCOPE | Client app UI/trust behaviour. |

## jmap-vtc-ring-push-guide.md (12)

| doc:line | MUST text (short) | verdict | evidence / bead / reason |
|---|---|---|---|
| vtc:9 | RFC 2119 keyword note | OUT OF SCOPE | Meta text. |
| vtc:26 | "the callee's device MUST ring within roughly two seconds" | OUT OF SCOPE | End-to-end latency across the deploying server's dispatch, the push platform and the mobile client. The kit has no delivery loop to time. Server ring semantics are tracked in JMAP-8mem.7 (closed). |
| vtc:113 | "the app MUST call `CXProvider.reportNewIncomingCall(...)` within approximately two seconds" | OUT OF SCOPE | iOS client app. |
| vtc:120 | "the app MUST call `reportNewIncomingCall` before any network round-trip" | OUT OF SCOPE | iOS client app. |
| vtc:151 | "The `apns-topic` for VoIP push MUST use the `.voip` suffix" | GAP (G2) | No APNs sender. The payload type exists (crate-jmap-vtc-types/src/push.rs:45 `VtcCallPush`), but nothing sets APNs headers. |
| vtc:217 | "MUST call `CXProvider.reportCall(with:endedAt:reason:)` to dismiss the CallKit UI" | OUT OF SCOPE | iOS client app. |
| vtc:267 | "the `VTCCallPush` JSON object MUST be JSON-serialized as a string in the `data` map" | GAP (G1) | No FCM envelope. |
| vtc:294 | "Apps targeting Android 9 (API 28) and later MUST declare and request this permission" | OUT OF SCOPE | Android client app manifest. |
| vtc:314 | "MUST call `Connection.setDisconnected` on the active `Connection`" | OUT OF SCOPE | Android client app. |
| vtc:500 | "Each device receiving `VTCCallEndEvent` with `endReason: "answered_elsewhere"` MUST stop ringing immediately" | OUT OF SCOPE | Client devices. The server-side `answered_elsewhere` dispatch is JMAP-8mem.7 (closed). |
| vtc:530 | "If implemented, the delay MUST be short (at most a few seconds)" | OUT OF SCOPE | Optional deployment policy (device-priority staggering), owned by the deploying server/operator. The kit implements no staggering (crate-jmap-vtc-server forks uniformly), so the rule never triggers. |
| vtc:670 | "the VoIP push delegate callback ... MUST be synchronous with respect to calling `reportNewIncomingCall`" | OUT OF SCOPE | iOS client app. |
