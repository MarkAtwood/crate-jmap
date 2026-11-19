# JMAP-x3lu: Scene/VTC guide MUST audit (jmap-chat-spec 120db2c)

Paths are relative to /data/PROJECT/crate-jmap. "Kit has no transport" = AGENTS.md L20 / crate-jmap-server/README.md L13: no HTTP/SSE/WS server transport in the workspace.

| doc:line | MUST text (short) | verdict | evidence / bead / reason |
|---|---|---|---|
| scene-wss:16 | RFC 2119 keyword boilerplate | OUT OF SCOPE | Defines keywords; no requirement |
| scene-wss:69 | Clients MUST account for dropped events when reading absence | OUT OF SCOPE | Client app logic (renderer/state model) |
| scene-wss:112 | Clients MUST NOT infer from absence of events | OUT OF SCOPE | Client app logic |
| scene-wss:123 | Clients MUST fetch Session and verify caps before WS | OUT OF SCOPE | Client app; kit supplies Session::primary_account_id (scene-client lib.rs:93-100 doc) but WsSession is not session-bound |
| scene-wss:134 | MUST NOT send SceneStreamEnable without scene:websocket cap | OUT OF SCOPE | Client app; send_scene_stream_enable (scene-client ws.rs:129) has no Session to check |
| scene-wss:245 | Clients MUST re-send full subscription state | OUT OF SCOPE | Client app. Server side full-replace semantics COVERED: scene-server ephemeral.rs:119 replace_with; test stream_enable.rs:39 subsequent_enable_replaces_in_entirety |
| scene-wss:256 | Server MUST silently succeed on Disable with no subscription | COVERED | scene-server ephemeral.rs:9-11,93-97 (Disable = reset to infallible Default); chat-scene-server tests/stream_union.rs:185 scene_stream_disable_leaves_chat_path. Transport frame handling is consumer's |
| scene-wss:273 | On reconnect clients MUST repeat full startup sequence | OUT OF SCOPE | Client app (kit leaves reconnect to caller, base-client ws/mod.rs:774-775) |
| scene-wss:319 | Clients MUST NOT persist avatar presence across reconnects | OUT OF SCOPE | Client app state |
| scene-wss:373 | Clients MUST ignore unrecognized action values | COVERED | Parse layer: scene-types event.rs:147 `action: String` accepts any value; UI handling is client renderer |
| scene-wss:378 | Clients not understanding action MUST ignore `data` | COVERED | Parse layer: event.rs:150 `data` opaque Map, never interpreted; app use is client's |
| scene-wss:411 | Clients MUST handle malformed frames gracefully | COVERED | scene-client ws.rs:45-57 (bad body -> Other(Unknown), never fails conn); base-client ws/mod.rs:466 returns Err(Parse) for non-JSON without closing; tests scene-client tests/ws.rs:231, base-client ws/mod.rs:1126 parse_invalid_json_returns_parse_error |
| scene-wss:428 | Clients MUST tolerate unknown fields in events | COVERED | scene-types event.rs:80,112,156 `#[serde(flatten)] extra` on all three events (no deny_unknown_fields). No dedicated extra-field test |
| scene-wss:465 | Server MUST NOT deliver duplicates (Chat+Scene union) | COVERED | chat-scene-server lib.rs:57 should_deliver_scene returns one bool per event/connection; tests stream_union.rs:140,158 |
| scene-wss:639 | Clients MUST handle both StateChange+event, MUST NOT treat as dups | OUT OF SCOPE | Client app; kit parser passes both through (ws.rs:29-35) |
| scene-wss:668 | Server MUST compute union with SceneStreamEnable and dedup | COVERED | chat-scene-server lib.rs:57-80; tests stream_union.rs:158 union_covers_what_either_path_covers, :196, :213 (JMAP-z996.4) |
| scene-wss:683 | Clients MUST always connect to wss:// | COVERED | base-client ws/mod.rs:950-958 scheme check in connect_ws; test ws/mod.rs:1219 connect_ws_rejects_plaintext_ws_by_default |
| scene-wss:684 | Clients MUST reject URL not beginning wss:// | COVERED | Same as :683; ws:// only via explicit connect_ws_allow_plaintext (ws/mod.rs:828) |
| scene-wss:724 | Clients MUST NOT reconnect with expired creds after 1008 | OUT OF SCOPE | Client app reconnect policy; kit surfaces 1008 distinctly as ClientError::WsClosed (base-client ws/mod.rs:469-477) and has no auto-reconnect |
| scene-wss:725 | Implementations MUST NOT log tokens/credentials from WS handshake | COVERED (client) / OUT OF SCOPE (server) | Client: base-client auth.rs:619-623 redacting Debug for AuthHeader; no logging in scene/vtc crates. Server handshake belongs to deployment transport (kit has no transport) |
| scene-impl:50 | RFC 2119 boilerplate | OUT OF SCOPE | No requirement |
| scene-impl:177 | Clients MUST NOT fail on unrecognized viewHint | COVERED | scene-types region.rs:9-29 ViewHint::Other(String); test region.rs:258 reverse_domain_view_hint_round_trips. Fallback rendering is client's |
| scene-impl:201 | accessPolicy "space" without Chat MUST be invalidProperties | COVERED | scene-server region.rs:384-386; test region_set.rs:376 space_policy_needs_the_chat_capability |
| scene-impl:241 | Servers MUST accept all three color forms and preserve | COVERED | environment is opaque object (scene-types region.rs:125; scene-server region.rs:331 accepts any object); round-trip test region_set.rs:148 (only scalars; no token/CAM16 value tested) |
| scene-impl:749 | Servers MUST support withinRadius and withinBounds | COVERED | scene-server query.rs:255-260, 288-293, 264-275; tests object_query.rs:120,148, avatar_query.rs:185 (JMAP-38uq.4) |
| scene-impl:886 | Server MUST accept model/gltf-binary | COVERED | scene-server asset.rs:42,294 (accepted even when capability omits it); test asset_set.rs:200 gltf_binary_is_mandatory_to_implement |
| scene-impl:1317 | Servers MUST return forbidden for SceneAvatar destroy | COVERED | scene-server avatar.rs:242-247; test avatar_set.rs:972 destroy_is_forbidden |
| scene-impl:1457 | EventSource is MUST | OUT OF SCOPE | Transport; kit has no SSE server (AGENTS.md L20). Deployment HTTP layer owns it; kit gives SCENE_DATA_TYPES + jmap-server state_change_for (scene-server push.rs:28, jmap-server push.rs:821) |
| scene-impl:1483 | 10+ Hz position MUST go via simulation layer, not JMAP push | OUT OF SCOPE | Architecture/deployment (simulation server behind simulationUri); kit has no position streaming path |
| scene-impl:1527 | StateChange for SceneAvatar MUST still be delivered (dedup with Chat) | COVERED | scene-server push.rs:77-82 lowers urgency to Low, never suppresses; test tests/push.rs:123 avatar_push_deduplicated_against_bound_chat_push. Actual send is consumer's |
| scene-impl:1729 | Checklist: EventSource support (MUST) | OUT OF SCOPE | Duplicate of :1457 (transport) |
| rec:15 | RFC 2119 boilerplate | OUT OF SCOPE | No requirement |
| rec:117 | invalidProperties naming state for unlisted transitions | COVERED | vtc-server media.rs:1111-1126 (atomic via update_object_if); test recording.rs:244 unlisted_transitions_are_invalid_state |
| rec:183 | supportsRecording and call-state checks MUST | COVERED | media.rs:554-557 (forbidden), 585-586 (invalidProperties callId), backend recheck backend.rs:222-226; tests recording.rs:154,169 |
| rec:324 | On call end server MUST stop every recording | COVERED | media.rs:686-712 stop_recordings, called from call.rs end_call (769); tests call_end_cascade.rs:169,215 (JMAP-8mem.9) |
| rec:446 | Server MUST NOT delete blob if other references exist | COVERED | vtc-server destroy removes metadata only; no blob deletion anywhere in vtc-server (only blobId null default media.rs:663). Blob GC is the deployment blob store. No test |
| rec:670 | Deployments MUST plan for recording size | OUT OF SCOPE | Ops/capacity planning |
| rec:1112 | Server MUST verify user has access to downloadUrl account | OUT OF SCOPE | RFC 8620 download endpoint is the deployment HTTP layer (kit has no download handler) |
| rec:1137 | Implementers MUST document cross-account approach | OUT OF SCOPE | Deployment documentation |
| vtc-impl:50 | RFC 2119 boilerplate | OUT OF SCOPE | No requirement |
| vtc-impl:250 | Re-join MUST reuse existing record regardless of time | COVERED | vtc-server memory.rs:1080-1090 alreadyExists+existingId (contract backend.rs:228-232); rejoin via leftAt:null participant.rs:817-841; tests participant.rs:270, 583 |
| vtc-impl:324 | muteOnEntry MUST set mediaState.audio false | COVERED | participant.rs:555-556; test participant.rs:333 entry_policy_mutes_regardless_of_the_request |
| vtc-impl:763 | MUST set activeCallId on Chat when call active | COVERED | chat-vtc-server lib.rs:269 sync_active_call_id; tests active_call_id.rs:35, 84 (JMAP-8mem.6) |
| vtc-impl:764 | MUST clear activeCallId when call ends | COVERED | lib.rs:269; test active_call_id.rs:54 ending_the_call_clears_active_call_id_with_state_change |
| vtc-impl:902 | Auto-end MUST deliver StateChange for VTCCall | COVERED | call.rs:769 end_call writes via update_object_if (records change in hosting account); ring timeout call.rs:842 expire_ring + VTCCallEndEvent; tests call_end_cascade.rs:215, updated_since helper :55 (JMAP-0x0q.67). Push fan-out is the consumer's transport |
| vtc-impl:912 | Child breakout calls MUST also be ended | COVERED | call.rs:733-737 cascade; tests call_end_cascade.rs:130,215,358 (JMAP-8mem.9, JMAP-8mem.14) |
| e2ee:14 | RFC 2119 boilerplate | OUT OF SCOPE | No requirement |
| e2ee:148 | MLS group membership MUST track active participants | OUT OF SCOPE | Client MLS stack / E2EE key management; server holds no keys |
| e2ee:312 | MUST surface E2EE constraints in client UI | OUT OF SCOPE | Client renderer |
| e2ee:384 | Breakout participant MUST NOT decrypt other rooms | OUT OF SCOPE | Client MLS (one group per room); server side each room is a separate VTCCall with own e2eeEnabled |
| gateway:46 | RFC 2119 boilerplate | OUT OF SCOPE | No requirement |
| gateway:428 | Client/gateway MUST ignore unrecognized signal | OUT OF SCOPE | Gateway and client app; vtc-types keeps `signal` an open String (event.rs:128) so kit parsing never rejects |
| scene-vtc:97 | Clients MUST check vtc capability before voice UI | OUT OF SCOPE | Client renderer |
| scene-vtc:122 | Clear SceneRegion.activeCallId on VTCCall end | COVERED | scene-vtc-server lib.rs:49 clear_active_call_id; tests active_call_id.rs:20,54,86 (JMAP-38uq.9) |
| 2d:54 | Clients MUST NOT fail on unrecognized viewHint | COVERED | scene-types region.rs:18-29 ViewHint::Other; test region.rs:258 |
| 2d:173 | Clients MUST NOT fail on non-zero ignored axis | OUT OF SCOPE (type system) | Position = [f64; 3] (coordinates.rs:13) accepts any value; clamping is client's |
| vtc-chat:91 | Clients MUST check vtc capability before calling UI | OUT OF SCOPE | Client renderer |
| vtc-deleg:144 | Server MUST set activeCallId on active, clear on end | COVERED | chat-vtc-server lib.rs:269; tests active_call_id.rs:35,54,84 |
| sim:53 | RFC 2119 boilerplate | OUT OF SCOPE | No requirement |

No GAPs found. Weak spots (COVERED but untested): scene-wss:428 extra-field tolerance, scene-impl:241 Design Token/CAM16 values in environment, rec:446 blob survival. None of these MUSTs touch livestream config; 901e1abb / e8bba2fa changes do not affect any row.
