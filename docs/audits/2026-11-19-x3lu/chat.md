# JMAP-x3lu: chat-family guide MUST audit (jmap-chat-spec @ 120db2c)

Paths: code under /data/PROJECT/crate-jmap. "OOS" = OUT OF SCOPE.
Boilerplate RFC 2119 lines and lines that only mention the word MUST are listed as OOS ("not a requirement").

## jmap-chat-wss-guide.md

| doc:line | MUST text (short) | verdict | evidence / bead / reason |
|---|---|---|---|
| wss:15 | RFC 2119 keyword boilerplate | OOS | Not a requirement. |
| wss:63 | Clients MUST account for silently dropped events | OOS | Client app (UI interpretation). The server-side drop rules are in chat-server ephemeral.rs:206-288. |
| wss:84 | Clients MUST NOT infer from absence of events | OOS | Client app. The kit's TypingIndicators (chat-client utils.rs:95) decays by timer and does not infer anything. |
| wss:96 | Fetch Session, verify both capabilities before connecting | OOS | Client app owns the startup flow. The kit gives `Session::websocket_capability` (base-client request.rs:792) and `supports_chat_websocket` (chat-client session.rs:207). |
| wss:108 | MUST NOT send ChatStreamEnable without chat:websocket cap | GAP | chat-client ws.rs:126 `send_stream_enable` sends with no capability check. The WsSession does not know the Session, so the kit cannot refuse the frame today. Proposed bead below. |
| wss:162 | MUST re-send ChatStreamEnable on navigation with explicit lists | OOS | Client app view logic. |
| wss:178 | On reconnect MUST repeat full startup sequence | OOS | Client app. The kit leaves reconnecting to the caller (base-client ws/mod.rs:213). |
| wss:192 | On 1008 MUST re-auth then reconnect | OOS | Client app. The kit's supporting part landed in JMAP-yuut.6: 1008 comes back as `WsClosed{code:1008}` (base-client ws/mod.rs:470-480), tested in tests/ws_close_code.rs. |
| wss:200 | MUST NOT reconnect with expired credentials | OOS | Client app (credential lifecycle). 1008 is surfaced as above. |
| wss:201 | MUST NOT clear stored pushState on 1008 | OOS | Client app storage. The kit stores no pushState (`send_push_enable(push_state: Option<&str>)`, ws/mod.rs:565). |
| wss:221 | MUST handle /changes response normally | OOS | Client app. |
| wss:222 | On cannotCalculateChanges MUST fall back to /get | OOS | Client app sync policy. |
| wss:226 | First login MUST omit pushState | OOS | Client app. The kit's `Option` parameter omits the field when it is `None` (ws/mod.rs:552-570). |
| wss:276 | MUST NOT gate Chat/typing on receiveTypingIndicators | COVERED | chat-client methods/chat.rs:178 `chat_typing` sends without reading the field. The app could still gate it; that part is the app's. |
| wss:281 | MUST NOT persist typing state across reconnects | OOS | Client app. The kit's TypingIndicators is in-memory only (utils.rs:95). |
| wss:296 | MUST apply ChatPresenceEvent as a delta | OOS | Client app state. The type keeps absent and null apart (chat-types ephemeral.rs:116-155, `Option<Clearable<_>>`). |
| wss:306 | MUST NOT rely on ephemeral stream to restore presence | OOS | Client app. |
| wss:360 | Server with no history for pushState MUST resume from current state, no replay | GAP | The only WS server in the kit is testjig src/ws.rs:494-498. An evicted token (`n+1 < oldest`) correctly resumes at current. But a token newer than the log, for example after a jig restart when `oldest==0`, sets `last_seen = n`. `events_since(n)` (replay.rs:185) then silently skips every new event with id <= n. No test covers either branch: tests/ws_resume.rs covers only the happy replay. Related closed bead: JMAP-cf7p.12. |
| wss:409 | Clients MUST always connect to wss:// | COVERED | base-client ws/mod.rs:943-951 (scheme check, `allow_plaintext` defaults to false). Tests: ws/mod.rs:1202 `connect_ws_rejects_non_ws_schemes`, :1219 `connect_ws_rejects_plaintext_ws_by_default`. Plaintext needs the explicit `connect_ws_allow_plaintext` opt-in. |
| wss:410 | MUST reject any URL not beginning wss:// | COVERED | Same code. `ClientError::InvalidArgument`. Tests at ws/mod.rs:1202-1261. |
| wss:415 | MUST NOT reconnect with expired creds / MUST re-auth first | OOS | Client app (same as wss:200). |
| wss:416 | MUST NOT log session tokens from WS handshake headers | COVERED | `AuthHeader` Debug prints `[REDACTED]` (base-client auth.rs:619-626). Tests: auth.rs:1297 `auth_header_debug_redacts_value`, tests/log_redaction.rs. The kit emits no tracing. The "re-authenticate first" clause on this line is OOS (client app). |

## jmap-chat-implementer-guide.md

| doc:line | MUST text (short) | verdict | evidence / bead / reason |
|---|---|---|---|
| impl:47 | RFC 2119 boilerplate | OOS | Not a requirement. |
| impl:257 | Server-admin actions MUST be audited to separate log | OOS | Deployment/ops: an audit sink outside the wire protocol. The kit has no logging. |
| impl:264 | Wire surface MUST remain identical with/without IdP | OOS | Deployment integration. The kit has no IdP integration, so its wire surface is IdP-independent. |
| impl:354 | "carve-out MUST clauses" | OOS | Not a requirement: it is an item in a "what to decide" list that refers to MUSTs elsewhere. |
| impl:400 | Deviations MUST be documented in API docs | OOS | Deployment documentation. |
| impl:466 | MUST reject CustomEmoji/set with overQuota on count/size limit | GAP | chat-server emoji.rs:440-485: the create path has no per-Space count or per-image size check. MemoryBackend `may_set_custom_emoji` (memory.rs:1995) returns Ok unconditionally. ChatLimits has no emoji caps. The only CustomEmoji cross-object check is name uniqueness (memory.rs:6100). No bead found: JMAP-n7v (closed) covered other limits but not emoji quota. |
| impl:543 | @here/@admins predicate MUST be evaluated at delivery time on each receiving server | COVERED | chat-server push.rs:205-228 `OwnerFlags` are supplied per delivery. push.rs:667 `mention_scopes(msg, owner)` resolves scopes when the push is built. memory.rs:5477-5530 resolves hasMention against current roles. Tests: tests/chat_push_payload.rs:378-391, tests/chat_push_should_push.rs. |
| impl:612 | Chat.id, Message.id, senderMsgId MUST be ULIDs | COVERED | chat-server memory.rs:230-268 (monotonic `ulid::Generator`). Test: testkit.rs:3545 `compliance_11_1_with_assigned_ids` (Crockford format plus ordering). Fixed by JMAP-nls1 (closed). Peer-supplied senderMsgId is not format-checked (deliver.rs:825), but it is the peer's id. |
| impl:624 | Peer servers MUST validate identity against documented format | OOS | The format is deployment-documented, so the kit cannot know a peer's regex. The kit enforces the spec's generic binding grammar: peer-auth identity.rs:47 `bind_identity`, used at federation deliver.rs:455. |
| impl:751 | ChatContact auto-creation MUST be rate-limited per sender/Space/hour | OOS | The kit has no auto-create path: chat-client creates only through explicit `chat_contact_create` (methods/contact.rs:119). Rate limits are consumer-owned, following the JMAP-yuut.4 decision. Owner: composer/deployment. |
| impl:889 | Edit-history retention MUST be documented in TOS | OOS | Deployment documentation. |
| impl:902 | Zero retention MUST be explicit in product copy | OOS | Product copy. |
| impl:977 | "rather than MUST mandates" | OOS | Not a requirement (prose). |
| impl:1027 | Deviating deployments MUST keep 3x decay/rate ratio | OOS | Applies only to deployments that change the values. The kit hardcodes the 10 s decay (chat-client utils.rs TYPING_DECAY). Rate limits are consumer-owned (JMAP-yuut.4). |
| impl:1030 | Deviations MUST be documented | OOS | Deployment documentation. |
| impl:1139 | After 24 h retry, message MUST be marked failed | OOS | Federation outbound.rs:7-8: "Queueing ... and the HTTP call stay with the deployment". The kit provides `is_transient` (outbound.rs:451) and `aggregate_delivery_state` (outbound.rs:407). The retry clock is the deployment queue's. |
| impl:1234 | Space-admin @everyone override MUST NOT be exposed via wire fields | OOS | Implied by the type system: chat-types has no such field. |
| impl:1237 | Default-bypass behaviour MUST be documented in privacy notice | OOS | Deployment documentation. |
| impl:1294 | Bidirectional receipt rule MUST be documented in plain language | OOS | Product copy. |
| impl:1297 | MUST trust federation receipt suppression at sender and receiver | COVERED | Receiver side: federation receipt.rs:36-91 (effective receiptSharing). Sender side: chat-server message.rs:1076-1106. Tests: federation tests/peer_methods.rs:194 `receipt_sharing_off_in_chat_keeps_only_delivered_at`, :224, :239. |
| impl:1298 | MUST NOT bypass with "unofficial read time" UI | OOS | Client UI. |
| impl:1366 | MUST NOT surface block status to blocked party | COVERED | Server wire side: federation deliver.rs:506 drops a blocked sender silently with a normal-looking response. Typing and presence are suppressed in chat-server ephemeral.rs:206-288. Test: federation tests/peer_deliver.rs:582 `step7_blocked_sender_is_dropped_silently`. UI side channels are the client's. |

## jmap-chat-did-guide.md

| doc:line | MUST text (short) | verdict | evidence / bead / reason |
|---|---|---|---|
| cdid:33 | boilerplate | OOS | Not a requirement. |
| cdid:583 | did:key deployments MUST plan for key loss | OOS | Deployment/ops planning. |
| cdid:781 | identity and did MUST refer to same DID | COVERED | chat-types contact.rs:214 `ChatContact::has_consistent_did`, test contact.rs:231. Clients cannot set `did`: ChatContact/set create rejects it (chat-server contact.rs:180-188). The kit never sets `did` itself. |
| cdid:925 | Rotation MUST be triggered on compromise | OOS | Ops procedure. |
| cdid:938 | Identity audit logging MUST capture sufficient detail | OOS | Deployment audit sink. The kit emits no logs. |
| cdid:943 | MUST NOT log private key material | OOS | Deployment logging. The kit holds no private keys in its own types: peer-auth keys.rs:15 keeps only VerifyingKey and rejects JWKs with `d`. The kit emits no tracing. |
| cdid:956 | Hardware-key deployments MUST document recovery | OOS | Deployment documentation. |

## jmap-chat-filenode-guide.md

| doc:line | MUST text (short) | verdict | evidence / bead / reason |
|---|---|---|---|
| cfn:26 | boilerplate | OOS | Not a requirement. |
| cfn:453 | Grace-period Spaces MUST count against quota | OOS | The kit offers no grace period: Space trees are destroyed immediately (chat-filenode-server lib.rs:298). A deployment that adds a grace period owns this. The quota check is `check_space_quota` (lib.rs:136-216). |
| cfn:635 | Legal hold MUST override destruction schedule | OOS | Operator-side compliance (the guide says it is not on the wire). The kit has no schedule. The deployment chooses whether to call the destroy helper from `on_space_destroyed`. |
| cfn:683 | Legal hold MUST suppress destruction-on-schedule | OOS | Same as cfn:635. |

## jmap-chat-calendars-guide.md

| doc:line | MUST text (short) | verdict | evidence / bead / reason |
|---|---|---|---|
| ccal:15 | boilerplate | OOS | Not a requirement. |
| ccal:71 | "Whether a Space MUST have a bound Calendar" | OOS | Not a requirement: an open deployment choice. |
| ccal:443 | Cross-server availability MUST have consent flow | OOS | Deployment. The kit has no federated availability lookup; Principal/getAvailability is local (calendars-server principal.rs:52). |
| ccal:497 | External adapter MUST keep event-ID mapping deterministic | OOS | The kit ships no external-provider adapter. Owner: adapter author. |

## jmap-chat-federation-guide.md

| doc:line | MUST text (short) | verdict | evidence / bead / reason |
|---|---|---|---|
| cfed:26 | boilerplate | OOS | Not a requirement. |
| cfed:510 | Deployments MUST set a Peer/deliver rate limit | OOS | Deployment transport. federation deliver.rs:259 documents that rate discarding "belongs here, with the transport". |

## jmap-chat-tasks-guide.md / jmap-chat-metadata-guide.md

| doc:line | MUST text (short) | verdict | evidence / bead / reason |
|---|---|---|---|
| ctask:15 | boilerplate | OOS | Not a requirement. |
| cmeta:15 | boilerplate | OOS | Not a requirement. |

## jmap-did-guide.md

| doc:line | MUST text (short) | verdict | evidence / bead / reason |
|---|---|---|---|
| did:165 | DID docs MUST be served as application/did+json or did+ld+json | OOS | Deployment web server. The kit does not serve DID documents. |
| did:409 | MUST reject client create with / update changing DID-bearing props | COVERED | jmap-server helpers.rs:1158 `reject_did_bearing_properties`, tests helpers.rs:2160-2246 (JMAP-0x0q.13 closed for the unchanged-value case). No kit data type is declared DID-bearing yet, so no handler calls the helper. ChatContact `did` (DID-referencing) is rejected on create by chat-server contact.rs:180-188. |
| did:435 | MUST reject with invalidProperties naming the property | COVERED | Same helper: it returns `invalidProperties` with `properties`. Tests helpers.rs:2175, 2233. |
| did:550 | On DID migration MUST update account capability | OOS | The account-to-DID binding store is the backend/deployment's (helpers.rs:1152: "Keeping the value unchanged ... across a DID migration, is the backend's job"). The type carries the field (did-types capability.rs:104-113). |
| did:551 | MUST set previousDid | OOS | Same as did:550. `previous_did` exists at did-types capability.rs:113. |

## jmap-cid-guide.md

| doc:line | MUST text (short) | verdict | evidence / bead / reason |
|---|---|---|---|
| cid:178 | Upload sha256 MUST be from bytes as received; MUST NOT advertise if transforming | OOS | The kit has no RFC 8620 HTTP upload endpoint, so the HTTP layer is the deployment's. Blob/upload in the memory backend hashes the stored bytes (blob-server memory/mod.rs:174, 403). Note: testjig advertises `jmap:cid` with no upload route (testjig session.rs:81-86, ponytail). That does not break this MUST, but it is a weak spot. |
| cid:199 | MUST NOT advertise cid when transforming content | OOS | Deployment decision on capability advertisement. |
| cid:290 | File FileNode sha256 MUST be blob's SHA-256 | COVERED | filenode-server filenode.rs:1194 (create) and :1951 (update blobId), taken from `blob_sha256`. Client-supplied values are rejected. Tests: tests/cid_sha256.rs:82, 134, 98, 194. |
| cid:372 | Knowing sha256 MUST NOT grant download access | OOS | Implied by the API surface: no kit method locates a blob by digest. Blob/get, Blob/lookup and download are keyed by blobId. FileNode sha256 is output-only (cid_sha256.rs:98). |
| cid:379 | Knowing hash MUST NOT give access to another user's blob | OOS | Same: there is no hash-keyed lookup path. |

## Proposed beads (GAPs)

1. **chat-client: refuse ChatStreamEnable when Session lacks chat:websocket.** Crate: crate-jmap-chat-client. Size: S. Oracle (wss:108): "clients MUST NOT send `ChatStreamEnable` in that case (a server that does not recognize it answers with a `RequestError` of type `notRequest`)".
2. **testjig WS: unknown or future pushState must resume from current, with tests.** Crate: crate-jmap-testjig (src/ws.rs:494). Size: S. Oracle (wss:360): "server MUST resume delivering from the current state without replaying history."
3. **chat-server: CustomEmoji/set overQuota for per-Space count and per-image size caps.** Crate: crate-jmap-chat-server (emoji.rs plus ChatLimits). Size: M. Oracle (impl:466): "The server MUST reject `CustomEmoji/set` with `overQuota` (per `{{RFC8620}}` §5.3) when the count or size limit is exceeded."
