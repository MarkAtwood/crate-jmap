# Research: VTC broadcast media backends (bd:JMAP-po6q.3, .4, .5)

Status: research write-up, decision 51 ("po6q.3-.5 broadcast research
written up (no adapter code)"). No crate code changed. Epic bd:JMAP-po6q;
spec epic jmap-chat-spec-xpzz (closed 2026-10-30, so the protocol smells
in §3.4 need a new spec bead or a reopen).

Spec text read: `draft-atwood-jmap-vtc-00.md` at jmap-chat-spec `3445973`
(§VTCRecording L542, §VTCLivestream L576-720, §VTCRecording/set L1275-1330,
§VTCLivestream/set L1373-1500). Crate code read: `crate-jmap-vtc-server`
(`src/backend.rs`, `src/media.rs`, README) and `crate-jmap-vtc-types`
(`src/recording.rs`).

Every web fact below was read on **2026-11-20** unless marked otherwise.
"Unconfirmed" means I looked and could not find a primary source.

## 0. The media hooks under test

`VtcBackend` has no dedicated media-plane trait. The media plane hangs off
these hooks and one missing piece:

| # | Hook | Media duty today |
|---|---|---|
| H1 | `create_object::<VtcLivestream>` | "A livestream backend then starts the outbound stream", in the same atomic step as the insert; also the per-account `overQuota` livestream limit |
| H2 | `create_object::<VtcRecording>` | start recording (implied; the doc comment only names the `active` recheck) |
| H3 | `update_object_if::<VtcLivestream>` | stop (moderator stop, call end via `media::stop_livestreams`, destroy via `destroy_media`); the handler adds `stoppedAt` and empties `playbackEndpoints` |
| H4 | `update_object_if::<VtcRecording>` | pause, resume, stop |
| H5 | `destroy_object` | the handler already stops first (`destroy_media`), so the backend only deletes metadata |
| H6 | `playback_endpoints_for` | per-viewer `url` / `credential` / `expiresAt` |
| H7 | `deliver_recording_state_event` | consent signal; no media work |
| H8 | `vtc_capability` | `supportsRecording`, `supportsLivestream`, `livestreamModes` |
| H9 | *(none)* | server-initiated transitions: livestream `starting`→`live`, →`failed` + `failureReason`, `playbackEndpoints` and `timeReference` appearing or changing, idle stop; recording `stopped`→`processing`→`available` (+ `blobId`, `size`, `duration`, `mediaType`) or `failed`. The backend has to write its own store and bump state itself |

Two crate-level findings fall out before any vendor is considered (they are
not protocol issues; they are for bd:JMAP-po6q.2):

- **Side effects inside the atomic step (H1-H4).** Every real pipeline call
  is slow and remote: IVS `StartComposition` is asynchronous and reports
  through EventBridge ([IVS SSC overview][ivs-ssc]), LiveKit egress reports
  `EGRESS_STARTING`→`EGRESS_ACTIVE` ([LiveKit Egress API][lk-api]), and the
  spec itself says "some media pipelines take minutes to provision". Under
  `register_vtc_conditional` an atomic `/set` can roll back after
  `create_object` returned; `MemoryBackend` rolls back state and queued
  `deliver_event` events, but nothing can roll back a started MediaLive
  channel. Starting or stopping media inside H1-H4 is therefore wrong for
  every backend, not just AWS.
- **H9 has no crate API.** All three backends below are event-driven
  (EventBridge, LiveKit webhooks, a child process exiting). Each one would
  re-implement "patch the livestream through `update_object_if` from
  `starting`, set `startedAt`, keep the change log and push right".

Both are addressed in Recommendations 1 and 2.

## 1. AWS (bd:JMAP-po6q.3)

### 1.1 MediaLive ingest: no WHIP/WebRTC

The MediaLive input-type list is: CDI, HLS, Link, MediaConnect, MediaConnect
Router, MP4, Multicast, RTMP Pull, RTMP Push, RTP, SDI, SMPTE 2110, SRT
Caller, SRT Listener, TS ([MediaLive input types][ml-inputs]). There is no
WHIP or WebRTC input, and I found no announcement of one for 2025-2026
(search of AWS docs and the web, 2026-11-20). WHIP itself is RFC 9725,
March 2025 ([RFC 9725][rfc9725]).

So a call that runs on a WebRTC SFU reaches MediaLive only after something
outside MediaLive (the §2 compositor) has turned it into **RTMP push**,
**SRT** (MediaLive has both SRT Caller and SRT Listener inputs, the caller
type dating from July 2024 per a third-party write-up, [Classmethod][cm-srt]),
or a **MediaConnect** flow (input by flow ARN, not URI,
[MediaLive input formats][ml-formats]). For one composited program feed,
SRT Listener or RTMP Push into a single-pipeline channel is the simplest.

### 1.2 Emulators

- **LocalStack:** the coverage list names Elemental MediaConvert among the
  media services and does not list MediaLive, MediaPackage, MediaConnect or
  IVS ([LocalStack coverage][ls-cov]). Separately, from March 2026 LocalStack
  ships one image that needs an auth token; CI runs consume credits, and
  hobbyists are told to pin an old Community tag such as 4.12, which gets
  no updates ([LocalStack blog][ls-road]). Unusable for this adapter.
- **Moto** (Python mocking library) implements the MediaLive control plane
  calls the adapter would make: `create_channel`, `create_input`,
  `start_channel`, `stop_channel`, `describe_*`, `list_*`, `update_*`,
  `delete_*` ([Moto MediaLive][moto-ml]); MediaPackage (v1) channels and
  origin endpoints ([Moto MediaPackage][moto-mp]); and IVS *low-latency*
  channels only (`create_channel`, `get_channel`, ...; no stream keys,
  no playback keys) ([Moto IVS][moto-ivs]). No `ivs-realtime` (stages,
  compositions) or MediaPackage v2 coverage was listed (unconfirmed absence).
  Moto runs as a stand-alone HTTP server, so a Rust test can point the AWS
  SDK endpoint at it. It mocks API state only: no media flows.
- **Conclusion:** no emulator exercises AWS media. CI for the AWS adapter
  can test the control-plane call sequence against Moto (MediaLive path) or
  a hand-written mock of `ivs-realtime`; media behaviour needs a real
  account (manual or nightly job). The free-tools backend (§3.3) is the
  thing that runs media offline in CI.

### 1.3 IVS Real-Time is the better managed option for calls

If the calls themselves run on AWS, IVS Real-Time stages beat
SFU→compositor→MediaLive→MediaPackage:

- **Ingest:** stages accept WHIP publishing with the participant token as
  the bearer token ([IVS WHIP publishing][ivs-whip]) and RTMP publishing
  ([IVS RTMP publishing][ivs-rtmp]), besides the broadcast SDKs.
- **Composition is built in.** Server-side composition (SSC) mixes all stage
  participants and sends the result to an IVS channel or an S3 bucket;
  layouts are grid and PiP with `featuredParticipantAttribute` and
  `participantOrderAttribute` driven by participant-token attributes, and the
  layout updates live on token exchange ([IVS SSC overview][ivs-ssc]). AWS
  warns layouts may change visually at any time.
- **Limits** ([IVS RT quotas][ivs-quotas]): 12 publishers per stage
  (adjustable), 10,000 subscribers per stage (adjustable), 12 video
  publishers per composition (not adjustable; extra publishers are audio
  only), 2 destinations per composition, 24 h max composition duration,
  720p max participant publish resolution. A composition auto-stops after
  60 s with no publisher ([IVS SSC overview][ivs-ssc]).
- **Recording:** composite recording writes HLS (media + manifests in an
  `hls` subfolder) to S3, not one file ([IVS composite recording][ivs-rec]).
  A `VTCRecording` needs one `blobId`, so the AWS adapter needs a remux step
  during `processing` (MediaConvert or ffmpeg).
- **No pause.** Compositions have Start/Stop only ([IVS SSC overview][ivs-ssc]).
- **Playback auth:** private IVS channels take a viewer JWT signed with the
  customer's playback key ([IVS private channels][ivs-priv]), which maps
  directly onto `playback_endpoints_for` (H6).
- **Sub-second playback:** stage subscribers are already sub-second, but
  subscribing is through the IVS SDKs; a WHEP subscribe URL is undocumented
  (one AWS dev post calls its method "undocumented" and liable to change,
  [dev.to Unity post][ivs-whep-undoc]). An AWS `interactive` endpoint is
  therefore a reverse-domain protocol (e.g. `com.amazonaws.ivs.stage`, the
  credential a subscribe-only participant token), which the spec allows.

If the calls run on our own SFU (the likely case for a JMAP deployment), the
AWS path is: §2 compositor → RTMP/SRT → (a) IVS low-latency channel, or (b)
MediaLive → MediaPackage → CloudFront. (a) is far less to build and gives
JWT playback auth; (b) is the route only when MediaPackage features (DRM via
SPEKE, DASH, DVR windows) are needed. Recommendation 4.

### 1.4 Cost note

All USD, on-demand, first volume tier, read 2026-11-20. The IVS page
carries no "last updated" date.

**IVS** ([IVS pricing][ivs-price]), North America and Europe rate:

| Item | Rate |
|---|---|
| Real-Time participant-hour (publisher or subscriber) | $0.0720 (audio-only $0.0072); South America $0.0840, Japan/SEA/India $0.0920, Australia $0.1000, South Korea $0.1250 |
| Server-side composition, per rendition-hour | SD $0.15, HD $0.30, Full HD $0.60 |
| Low-latency channel input, per hour | Basic $0.20, Advanced SD $0.50, Advanced HD $0.85, Standard $2.00 |
| Low-latency output, per hour delivered | SD $0.0360, HD $0.0720, Full HD $0.1440 |

The billing region is where each participant connects or where viewers are
served from ([IVS pricing][ivs-price]).

Worked example (my arithmetic, North America): a one-hour, four-person call
on a stage, one HD composition to a Basic channel, 100 HD viewers:
4 × 0.072 + 0.30 + 0.20 + 100 × 0.072 = **$7.99**. Viewer-hours dominate,
and a viewer subscribed straight to the stage costs the same $0.072/hour as
an HD low-latency viewer, while getting sub-second latency (up to the
10,000-subscriber quota). Whether SSC into an IVS channel also bills the
channel input is unconfirmed; the example assumes it does.

**MediaLive** ([MediaLive pricing][ml-price], US East (N. Virginia)): the
page shows rates only inside worked examples. Single-pipeline output
720p or 1080p HD AVC 30 fps $0.4212/hour (standard dual-pipeline $0.702);
input 1080p HEVC 20 Mbps $0.3528/hour single-pipeline. The AVC input rate and
the idle-channel rates are not visible in the fetched page (unconfirmed).
Billing is per minute with a 10-minute minimum; push inputs and channels
incur idle charges. So a single-pipeline HD channel with one HD output is
roughly $0.75-0.80/hour before MediaPackage (estimate; AVC input rate not
confirmed).

**MediaPackage** ([MediaPackage pricing][mp-price], US East): ingest
$0.030/GB, origination $0.050/GB, no per-channel-hour charge, plus data
transfer out. A 5 Mbit/s feed is about 2.25 GB/hour ingested ($0.07/hour).

**LiveKit Cloud** for comparison with §3.2 ([LiveKit pricing][lk-price]):
egress (transcoded) $0.02/minute video overage on Build/Ship
($1.20/hour), $0.015/minute on Scale; WebRTC connection minutes
$0.0005/minute on Ship ($0.03/participant-hour).

## 2. Compositing a call into one program feed (bd:JMAP-po6q.4)

The problem: a call is N WebRTC tracks on an SFU. Hosted and push
livestreams, and recordings, need **one** encoded audio+video feed (or one
file), with a layout, pushed over RTMP/SRT/WHIP to the packager or written to
disk.

### 2.1 Survey

| Option | Inputs | Layout control | Outputs | Latency added | Licence | Operational weight |
|---|---|---|---|---|---|---|
| **LiveKit Egress** (RoomComposite / Web) | a LiveKit room (or any URL for Web egress) | headless Chromium renders a web template: built-in `grid`, `speaker`, `single-speaker`, or a custom HTML template; `UpdateLayout` on a live egress ([LiveKit composite][lk-comp], [LiveKit Egress API][lk-api]) | MP4 file, HLS segments, RTMP, SRT; one output per type per job; `UpdateStream` adds/removes stream URLs ([LiveKit outputs][lk-out]); no WHIP output listed | no figure published (unconfirmed); "RTMP streams do not perform well over long distances" ([LiveKit outputs][lk-out]) | Apache-2.0 ([livekit/egress][lk-egress]) | heavy: Chrome + GStreamer + Xvfb + PulseAudio per job, shares the LiveKit server's Redis; relative `room_composite_cpu_cost` 3.0 ([livekit/egress][lk-egress]). No pause RPC ([LiveKit Egress API][lk-api]) |
| **Jitsi Jibri** | a Jitsi Meet conference | whatever the Jitsi Meet web UI shows | live stream (stream key prompt) or recording | unconfirmed | Apache-2.0 | heaviest: Chrome in a virtual framebuffer, ffmpeg capture, ALSA loopback; "Only one recording at a time is supported on a single jibri", dedicated VM ([jitsi/jibri][jibri]) |
| **Janus** | Janus rooms | none for video: Janus does no mixing ("muxing, mixing, and transcoding are left to third-party applications"); AudioBridge mixes audio live, records the mix, and RTP-forwards it ([Janus recordings][janus-rec], [Janus AudioBridge][janus-ab]) | per-stream `.mjr` files, post-processed by `janus-pp-rec` without transcoding ([Janus recordings][janus-rec]); RTP forwarders to an external mixer | n/a (needs an external mixer) | GPL-3.0 ([janus-gateway][janus-gh]) | light core, but the compositor is still ours to build |
| **mediasoup** | mediasoup routers | none: "just" an SFU, no audio mixing, no transcoding ([mediasoup FAQ][ms-faq]) | RTP to an external process; a `mediasoup-recording` demo exists ([Kurento mediasoup-demos][ms-demos]; its internals unconfirmed) | n/a | ISC ([mediasoup][ms-gh]) | as Janus |
| **Pion** (Go library) | anything we build | none built in | examples receive into GStreamer/FFmpeg, save to WebM, send to Twitch over RTMP ([pion examples][pion-ex]) | n/a | MIT ([pion/webrtc][pion-gh]) | a library; the mixer is still GStreamer or FFmpeg |
| **IVS server-side composition** | IVS stage participants only | grid / PiP, featured and ordered by token attributes ([IVS SSC overview][ivs-ssc]) | IVS channel, S3 (HLS) | unconfirmed | managed | none to run; 12 video tiles, 24 h cap ([IVS RT quotas][ivs-quotas]) |
| **GStreamer** `compositor` + `audiomixer` | any GStreamer source: RTP, SRT, WHEP (`whepsrc`, `whepclientsrc`), `webrtcsrc`, LiveKit (`livekitwebrtcsrc`) ([webrtchttp][gst-whep], [rswebrtc][gst-rswebrtc]) | per-pad `xpos`, `ypos`, `width`, `height`, `alpha`, `zorder`, `sizing-policy`; pads requested at runtime; `ignore-inactive-pads` for live pads ([compositor][gst-comp]). That the pad properties are safe to change while playing is implied by the docs (animating them) but not stated (unconfirmed) | `rtmp2sink`, `srtsink`, `whipclientsink`, `hlssink2`, `dashsink` ([rtmp2sink][gst-rtmp2], [srtsink][gst-srt], [whipclientsink][gst-whip], [hlssink2][gst-hls], [dashsink][gst-dash]); `valve` can drop buffers on demand ([valve][gst-valve]), which gives recording pause | sub-second is achievable for a live mix (no published figure; unconfirmed) | LGPL ([GStreamer licensing][gst-lic]); gst-plugins-rs elements are separate crates | medium: one long-lived pipeline per livestream, a Rust binding (`gstreamer-rs`) fits this workspace's language |
| **FFmpeg** `xstack` / `overlay` + `amix` | any FFmpeg input | `xstack` takes a fixed `layout` or `grid` at graph build; it has no runtime commands, while `amix` does (`weights`, `normalize`) ([FFmpeg filters][ff-filters]) | RTMP, SRT, HLS, DASH, and since 8.0 an *experimental* WHIP muxer (H.264 without B-frames + Opus; the doc's example gets ~150 ms end-to-end; `authorization` takes the bearer token) ([FFmpeg formats][ff-formats], FFmpeg 8.0 "Formats: MCC, G.728, Whip, APV" per [LWN][ff8]) | low for a fixed layout; a join or leave means rebuilding the graph (a visible glitch) | LGPL-2.1+ (GPL with some options) ([FFmpeg legal][ff-legal]) | light per process; best for fixed-N tiles, post-processing (remux, concat) and CI test sources |
| **Headless browser** (Playwright/Puppeteer + Xvfb + capture, or tab capture → WebRTC → WHIP) | anything a web page can show: SFU tracks, overlays, an in-world scene renderer | arbitrary HTML/CSS/WebGL | whatever the capture pipeline pushes | highest of the live options (browser render + capture + encode; unconfirmed figure) | components are permissive; product-level licence is ours | heaviest: one browser per feed. LiveKit Web egress and Jibri are productised versions |

Also relevant as an **origin and WHEP server** (not a compositor):
MediaMTX (MIT) accepts SRT, WebRTC, RTMP, RTSP and more, serves
WebRTC/WHEP (`/<path>/whep`), HLS including low-latency HLS, SRT and RTMP,
and authenticates readers internally, by HTTP callback, or by JWT checked
against a JWKS; for WebRTC and HLS the JWT goes in `Authorization: Bearer`
([MediaMTX README][mtx-readme], [MediaMTX WebRTC read][mtx-webrtc],
[MediaMTX HLS read][mtx-hls], [MediaMTX authentication][mtx-auth]). DASH is
not listed (unconfirmed absence). The fallback for browsers that cannot set
the header is described on the auth page but I did not confirm the exact
query parameter (unconfirmed).

### 2.2 Recommended scope and where it lives

- **Outside the protocol crates.** The compositor is a media service, not a
  JMAP concern. `jmap-vtc-server` should only see "start a program feed for
  call X with layout L to destination D, report its state". Put the
  compositor in its own repository or a non-published workspace member
  (for example `tools/vtc-compositor`), with no dependency on
  `jmap-vtc-server`; adapters (§3) talk to it over a small control API.
- **Scope for a first version:** tile layouts only (`grid`, `speaker`,
  `single-speaker`, the three names LiveKit and IVS both roughly cover),
  audio mix of all unmuted participants, one output URL (RTMP, SRT or WHIP),
  plus a file sink for recordings, plus pause by `valve`.
- **Implementation choice by deployment SFU:**
  - LiveKit SFU: use LiveKit Egress as-is; write nothing.
  - IVS stages: use SSC as-is; write nothing.
  - Any other SFU (Janus, mediasoup, Pion, our own): a GStreamer pipeline via
    `gstreamer-rs`: SFU RTP forwards (or `whepsrc` per participant) →
    `compositor` + `audiomixer` → encoder → `whipclientsink` / `srtsink` /
    `rtmp2sink` / `filesink`. GStreamer over FFmpeg because pads can be
    added and moved live; FFmpeg `xstack` cannot re-layout without a restart.
  - The headless browser is only for the in-world (jmap-scene) renderer,
    and is a later, separate item.
- **Out of scope:** E2EE calls (the handlers already refuse; a key-holding
  bridge is its own project), DRM packaging, transcoding ladders (the
  packager's job), captions generation.

## 3. Backends against the hooks (bd:JMAP-po6q.5)

The sketches assume Recommendations 1-2: H1-H4 only persist and enqueue,
media work happens after commit, and server-initiated changes go through a
reporter. Without them the same code would sit inside `create_object` /
`update_object_if`, which §0 rules out. Pseudo-Rust; names are
illustrative, not proposals for public API spelling.

```rust,ignore
/// Proposed (Recommendation 1): called by the handlers after the /set commits.
trait VtcMediaPlane {
    async fn start_livestream(&self, ctx: &Ctx, account: &Id, ls: &VtcLivestream);
    async fn stop_livestream(&self, ctx: &Ctx, account: &Id, ls: &VtcLivestream);
    async fn start_recording(&self, ctx: &Ctx, account: &Id, rec: &VtcRecording);
    async fn set_recording_paused(&self, ctx: &Ctx, account: &Id, rec: &VtcRecording, paused: bool);
    async fn stop_recording(&self, ctx: &Ctx, account: &Id, rec: &VtcRecording);
}

/// Proposed (Recommendation 2): crate helpers that go through
/// update_object_if with the right source states, so state strings,
/// change logs and push stay correct.
mod media_report {
    async fn livestream_live(b, account, id, endpoints: Vec<VtcPlaybackEndpoint>, started_at);
    async fn livestream_endpoints(b, account, id, endpoints);       // e.g. timeReference reset
    async fn livestream_failed(b, account, id, reason: LivestreamFailureReason);
    async fn livestream_idle_stop(b, account, id);
    async fn recording_processing(b, account, id, duration);
    async fn recording_available(b, account, id, blob_id, size, duration, media_type);
    async fn recording_failed(b, account, id);
}
```

### 3.1 AWS (reference column; the adapter itself is bd:JMAP-po6q.2)

Calls on our SFU: §2 compositor → SRT Listener input of a single-pipeline
MediaLive channel → MediaPackage → CloudFront, or → IVS low-latency channel.
Calls on IVS stages: `StartComposition` with an IVS channel destination
(hosted), S3 destination (recording). State from EventBridge; IVS viewer JWTs
or CloudFront signed URLs in H6.

### 3.2 (a) WHEP / SFU-egress backend

LiveKit has no WHEP playback in its own docs: ingress takes RTMP, WHIP, SRT
and HTTP files, egress produces MP4, HLS, RTMP and SRT
([LiveKit ingress/egress][lk-io]); a third-party post says LiveKit lacks WHEP
([OpenVidu blog][ov-whip]). So the sketch pairs LiveKit Egress with
MediaMTX as the WHEP (and low-latency HLS) origin. Swap LiveKit for the §2
GStreamer compositor on another SFU; the hooks do not change.

```rust,ignore
struct WhepSfuBackend<S> {
    store: S,                       // the JMAP store (any JmapBackend)
    livekit: LiveKitClient,         // Egress API + room service
    mtx_base: Url,                  // https://media.example.com
    mtx_srt: Url,                   // srt://media.example.com:8890
    jwt: Es256Signer,               // key whose JWKS MediaMTX trusts (authJWTJWKS)
    jobs: JobTable,                 // livestream/recording id -> egress id
}

impl<S: JmapBackend> VtcBackend for WhepSfuBackend<S> {
    fn vtc_capability(..) -> VtcCapability {
        // supportsLivestream: true, livestreamModes: ["push", "hosted"],
        // supportsRecording: true
    }

    // H1/H2: persist only (state "starting" / "recording"), enforce the
    // livestream overQuota limit atomically. No LiveKit call here.
    fn create_object<O, F>(..) { self.store.insert_atomic(..) }

    // H3/H4/H5: persist only; the handler already built the patch.
    fn update_object_if<O>(..) { self.store.cas(..) }
    fn destroy_object<O>(..)   { self.store.delete(..) }

    // H6: per-viewer, short-lived.
    async fn playback_endpoints_for(&self, caller, account, id, stored) -> Vec<VtcPlaybackEndpoint> {
        if !self.may_view(caller, account, id).await? { return Ok(vec![]); }  // §Playback Authorization
        let token = self.jwt.sign(Claims { sub: user_id(caller), path: format!("ls/{id}"),
                                           action: "read", exp: now() + 5.min() });
        stored.into_iter().map(|mut ep| {
            ep.credential = Some(token.clone());   // MediaMTX reads it as Authorization: Bearer (WebRTC and HLS)
            ep.expires_at = Some(now() + 5.min());
            ep
        }).collect()
    }

    fn deliver_recording_state_event(..) { self.ws.fan_out(..) }     // H7: no media work
}

impl VtcMediaPlane for WhepSfuBackend<_> {
    async fn start_livestream(&self, _, account, ls) {
        let dest = match ls.mode {
            Hosted => self.mtx_srt.with_stream_id(format!("publish:ls/{}", ls.id)),
            Push   => push_url(ls.stream_uri, ls.stream_key),   // rtmp(s)://, srt://
        };
        let egress = self.livekit.start_egress(RoomComposite { room: ls.call_id,
                                               layout: "speaker", stream: dest }).await;
        self.jobs.put(ls.id, egress);
    }
    async fn stop_livestream(&self, _, _, ls) { self.livekit.stop_egress(self.jobs.get(ls.id)).await }
    async fn start_recording(&self, _, _, rec) {
        self.livekit.start_egress(RoomComposite { room: rec.call_id, file: mp4_to(&rec.id) }).await;
    }
    async fn set_recording_paused(..) { /* no pause RPC: stop + new egress, concat at the end
                                           (or refuse; see smell S1) */ }
    async fn stop_recording(&self, _, _, rec) { self.livekit.stop_egress(..).await }
}

// LiveKit webhook egress_started / egress_updated / egress_ended -> H9
async fn on_egress_event(b: &WhepSfuBackend<_>, info: EgressInfo) {
    match (b.jobs.kind(&info.egress_id), info.status) {
        (Livestream(id, Hosted), EGRESS_ACTIVE) => media_report::livestream_live(b, acct, id, vec![
            VtcPlaybackEndpoint::new(Whep, b.mtx_base.join(&format!("ls/{id}/whep")), Interactive),
            VtcPlaybackEndpoint::new(Hls,  b.mtx_base.join(&format!("ls/{id}/index.m3u8")), Low),
        ], now()).await,
        (Livestream(id, Push), EGRESS_ACTIVE) => media_report::livestream_live(b, acct, id, vec![], now()).await,
        (Livestream(id, _), EGRESS_FAILED)    => media_report::livestream_failed(b, acct, id,
                                                    if was_live { Interrupted } else { Rejected }).await,
        (Livestream(id, _), EGRESS_LIMIT_REACHED) => media_report::livestream_failed(b, acct, id, Interrupted).await,
        (Recording(id), EGRESS_COMPLETE) => { let blob = b.blobs.import(info.file).await;
                                              media_report::recording_available(b, acct, id, blob, ..).await }
        (Recording(id), EGRESS_FAILED)   => media_report::recording_failed(b, acct, id).await,
        _ => {}
    }
}
```

Event names and statuses: [LiveKit webhooks][lk-hooks],
[LiveKit Egress API][lk-api]. `timeReference` for the WHEP endpoint stays
`null` (each WHEP session has its own RTP timeline; the spec points clients
at RTCP sender reports). Room-composite egress ends when all participants
leave ([LiveKit composite][lk-comp]), which maps onto the spec's idle stop.

### 3.3 (b) Free-tools reference backend (offline CI)

No network, no accounts, no DRM. In CI the "call" is a synthetic source, so
the test needs no SFU; in a dev deployment the same code takes the SFU's RTP
forward or the §2 compositor's output instead.

```rust,ignore
struct FreeToolsBackend {
    store: MemoryBackend,            // reuse the reference store
    root: PathBuf,                   // static origin directory
    origin: Url,                     // http://127.0.0.1:<port>/ served by tower-http ServeDir in the test
    procs: Mutex<HashMap<Id, Child>>,
    source: SourceKind,              // TestPattern (CI) | Rtp { port } (dev)
}

impl VtcMediaPlane for FreeToolsBackend {
    async fn start_livestream(&self, _, account, ls) {
        let out = self.root.join(ls.id.as_str());
        let child = match ls.mode {
            // gst-launch-1.0 videotestsrc is-live=true ! x264enc tune=zerolatency ! h264parse !
            //   hlssink2 location=<out>/seg%05d.ts playlist-location=<out>/index.m3u8
            // (or ffmpeg -re -f lavfi -i testsrc2 -f lavfi -i sine ... -f hls
            //   -hls_flags program_date_time <out>/index.m3u8, and -f dash for DASH)
            Hosted => spawn_hls(&self.source, &out),
            // ffmpeg ... -f flv rtmp://... | -f mpegts srt://... | -f whip -authorization <key> https://...
            Push => spawn_push(&self.source, ls.stream_uri, ls.stream_key),
        };
        self.procs.lock().insert(ls.id.clone(), child);
        // watcher task: first playlist file appears -> live; child exits non-zero -> failed
        spawn(watch(self.clone(), account.clone(), ls.id.clone(), out));
    }
    async fn stop_livestream(&self, _, _, ls) {
        // SIGINT so the muxer writes EXT-X-ENDLIST, then wait
        self.procs.lock().remove(&ls.id).map(graceful_stop);
    }
    async fn start_recording(&self, _, _, rec) { /* gst: ... ! valve name=v ! webmmux ! filesink */ }
    async fn set_recording_paused(&self, _, _, rec, paused) { /* set valve drop=paused (native) */ }
    async fn stop_recording(&self, _, account, rec) {
        // EOS, wait; then media_report::recording_processing, store the file as a blob,
        // media_report::recording_available(.., "video/webm")
    }
}

async fn watch(b, account, id, out) {
    match first_of(file_appears(out.join("index.m3u8")), child_exit(&id), timeout(30.s())).await {
        Appeared => media_report::livestream_live(&b, &account, &id, vec![{
            let mut ep = VtcPlaybackEndpoint::new(Hls, b.origin.join(&format!("{id}/index.m3u8")), Standard);
            ep.time_reference = Some(VtcTimeReference::new(0, pipeline_start_wall_clock));
            ep                                           // credential None, expiresAt None
        }], now()).await,
        Exited(code) => media_report::livestream_failed(&b, &account, &id, Rejected).await,
        TimedOut     => media_report::livestream_failed(&b, &account, &id, Timeout).await,
    }
    // later exit while live -> livestream_failed(Interrupted)
}

impl VtcBackend for FreeToolsBackend {
    // H1-H5: delegate to MemoryBackend (persist only)
    // H6: the default (stored list, no auth)
    // H7: MemoryBackend's queue
}
```

The CI test then runs the real handlers: create a hosted livestream, wait for
`state: "live"` via `VTCLivestream/get`, fetch the playlist from `origin`,
check a segment arrives, stop, check `EXT-X-ENDLIST` and empty
`playbackEndpoints`. It needs `gst-launch-1.0` or `ffmpeg` on the CI image
(neither is installed on this host today), so gate it behind a feature or
an `#[ignore]` with a CI job that installs them.

### 3.4 Fit of every hook against the three backends

Y = fits as is; P = fits with extra work the hook does not show; N = does
not fit.

| Hook / spec element | AWS | WHEP/SFU | Free tools | Note |
|---|---|---|---|---|
| H1 `create_object` livestream (incl. start "in the same atomic step") | N | N | P | starting media in the atomic step is wrong for all three (§0); fits all once Recommendation 1 moves it after commit. **Crate** smell |
| H1 per-account `overQuota` livestream limit | Y | Y | Y | |
| H2 `create_object` recording | P | Y | Y | AWS composite recording is HLS in S3, so `available` needs a remux during `processing` |
| H3 stop livestream (moderator, call end, destroy) | Y | Y | Y | after Recommendation 1 |
| H4 recording pause/resume | P | P | Y | IVS compositions and LiveKit egress have no pause; only GStreamer pauses natively (`valve`). See **S1** |
| H4 recording stop → `processing` → `available` | P | Y | Y | AWS remux; all need H9 |
| H5 `destroy_object` | Y | Y | Y | the handler stops first |
| H6 `playback_endpoints_for` | Y | Y | Y | IVS viewer JWT or CloudFront signed URL; MediaMTX JWT; free tools return `stored` |
| H7 `deliver_recording_state_event` | Y | Y | Y | no media dependence |
| H8 capability flags | Y | Y | Y | but see S2: push protocols are not advertised |
| H9 server-initiated transitions | P | P | P | no crate API; Recommendation 2 |
| `mode: "push"` `streamUri` + `streamKey` | P | Y | Y | IVS SSC cannot push to arbitrary RTMP (destinations are IVS channel and S3, [IVS SSC overview][ivs-ssc]), so AWS push needs MediaLive or our compositor. See **S2** |
| `mode: "hosted"` create with `callId` + `mode` only | Y | Y | Y | but no layout input: see **G1** |
| `latencyHint` | Y | Y | Y | |
| `timeReference` | Y | P | Y | HLS: from program date-time; WHEP sessions have per-session timelines, so `null` (allowed) |
| `tracks` (audio languages, captions) | Y | P | Y | WebRTC has no standard caption track; empty list is allowed |
| `encrypted` / `keySystems` | Y | Y (false) | Y (false) | optional; only AWS can set `true` (MediaPackage SPEKE, not researched here), which is fine for an optional field |
| `failureReason` values | Y | Y | Y | EventBridge destination failure / `EGRESS_FAILED` / child exit map onto `rejected`, `interrupted`, `unavailable`, `timeout` |
| idle stop | Y | Y | P | IVS auto-stops after 60 s without publishers; LiveKit room composite ends when everyone leaves; free tools must detect silence itself |
| `starting` unbounded duration | Y | Y | Y | |

**Hooks that fit only one backend (protocol smells for jmap-chat-spec-xpzz):**

> **Resolved 2026-11-20 (Mark, bd:JMAP-4j03):** S1, S2 and G1 below were
> adopted as suggested in jmap-chat-spec 125749a (`supportsRecordingPause`,
> `livestreamPushSchemes` with stream key presentation, `layouts` and the
> `layout` property) and implemented in jmap-vtc-types and
> jmap-vtc-server.

- **S1. Recording pause/resume** (`"recording"` ⇄ `"paused"`, §Pausing,
  Resuming, and Stopping a Recording). Native only in the free-tools
  (GStreamer) backend. AWS and LiveKit must emulate it: stop the composition
  or egress, start a new one on resume, and concatenate the pieces during
  `processing`, which costs a restart gap and a post-processing pass, and on
  IVS a new composition each time. The spec makes the transition mandatory
  with no way to decline it short of `invalidProperties`, which clients would
  read as a client error.
  *Suggested change:* add an account-level capability flag
  `supportsRecordingPause` (Boolean, default `true` so existing text keeps
  its meaning); when `false` the server returns `invalidProperties` naming
  `state` for `"paused"`, and clients hide the pause control. Add one
  sentence that a server MAY implement pause by omitting the paused interval
  from the file, so emulation by segment-and-concatenate is conformant.

That is the only strict "fits one of three" hook I found. Two near-misses
are worth raising in the same bead, because they fit all three only by
undocumented convention:

- **S2. Push-mode protocol shape** (`streamUri` "RTMP or equivalent",
  `streamKey`). All three backends can push RTMP, but they differ on SRT and
  WHIP (MediaLive has no WHIP; LiveKit egress has no WHIP output; FFmpeg 8.0
  and GStreamer push WHIP). A client cannot learn which schemes a server
  accepts, and the spec does not say where `streamKey` goes for non-RTMP
  protocols. *Suggested change:* add account-level `livestreamPushSchemes`
  (String[], URI schemes such as `"rtmp"`, `"rtmps"`, `"srt"`, `"https"` for
  WHIP), return `invalidProperties` naming `streamUri` for an unlisted
  scheme, and say that `streamKey` is presented in the protocol's own way:
  appended as the RTMP stream key, as the SRT `streamid` or passphrase per
  the destination's convention, as the WHIP bearer token (RFC 9725).
- **G1. No layout input for hosted (or push) livestreams and recordings.**
  Every backend has a layout control (IVS grid/PiP + featured participant;
  LiveKit `grid`/`speaker`/`single-speaker` + `UpdateLayout`; GStreamer
  anything), but the protocol gives the moderator none, so the choice is
  fixed per deployment. *Suggested change (optional):* an optional,
  updatable `layout` String on VTCLivestream and VTCRecording from an open
  set (`"grid"`, `"speaker"`, `"single-speaker"`; reverse-domain for others;
  unknown values `invalidProperties`), and an account-level `layouts`
  list. Or record explicitly that layout is deployment policy. This is a
  feature gap, not a smell; Mark's call (open question 3).

## Recommendations

1. **Move media side effects after commit** (bd:JMAP-po6q.2 prerequisite,
   crate change to `jmap-vtc-server`): H1-H4 persist only; add a post-commit
   media hook set (sketched as `VtcMediaPlane`) the handlers call after a
   `/set` commits, like `deliver_event`; update the `create_object` doc,
   which today says the backend "starts the outbound stream" in the atomic
   step.
2. **Add server-initiated transition helpers** (`media_report::*`) in
   `jmap-vtc-server`, built on `update_object_if` with the spec's source
   states, so every backend reports `live` / `failed` / endpoints /
   `available` the same way.
3. **Raise S1 (and S2, G1) on the spec** as a new bead under (or reopening)
   jmap-chat-spec-xpzz, with the suggested changes above.
4. **AWS adapter target:** if calls run on our SFU, compositor → SRT/RTMP →
   IVS low-latency channel first (JWT playback auth, little to build);
   MediaLive + MediaPackage only when DRM, DASH or DVR is needed. If a
   deployment runs calls on IVS stages, use SSC and stage subscribe tokens.
   Test the control plane against Moto (MediaLive/MediaPackage) or a hand
   mock (`ivs-realtime`); LocalStack is out.
5. **Compositor lives outside the protocol crates**: LiveKit Egress or IVS
   SSC where the SFU provides one; otherwise a GStreamer (`gstreamer-rs`)
   tile compositor in its own repo or tool crate. Headless browser only for
   the later in-world renderer.
6. **Build the free-tools backend first** (after 1 and 2): it is the CI
   oracle for the hooks and needs only ffmpeg or GStreamer plus a static
   file server; then the WHEP/SFU backend (LiveKit + MediaMTX); then AWS.

## Open questions for Mark

1. Which SFU do the reference deployments run (LiveKit, Janus, mediasoup,
   our own)? That decides whether the compositor is "use LiveKit Egress" or
   "build the GStreamer one".
2. Is a CI job that installs GStreamer or ffmpeg acceptable, or must the
   free-tools test be `#[ignore]` by default?
3. Layout (G1): protocol property, or deployment policy?
4. Recording pause (S1): capability flag as suggested, or keep pause
   mandatory and accept emulation cost on AWS and LiveKit?
5. For the AWS adapter, is an IVS low-latency channel acceptable as the
   "AWS packager pipeline" of the design test, or does the test need
   MediaLive + MediaPackage specifically?
6. Should the compositor be a new repository or a non-published member of
   this workspace?

## Unconfirmed

- MediaLive WHIP/WebRTC: absent from the input-type list and no
  announcement found; absence, not a statement from AWS.
- MediaLive AVC input rates and idle-channel rates (not visible in the
  fetched pricing page); whether SSC into an IVS channel bills channel input.
- IVS WHEP subscribe (undocumented); IVS composition latency.
- LiveKit WHEP playback (absent from LiveKit docs; third-party sources
  conflict); LiveKit egress pause (no RPC documented); egress latency;
  units of `room_composite_cpu_cost`.
- GStreamer `compositor` pad changes while playing (implied, not stated);
  live-mix latency figures for GStreamer, browser renderers, Jibri.
- MediaMTX query-parameter fallback for JWT; DASH support.
- `mediasoup-recording` demo internals; Moto coverage of `ivs-realtime` and
  MediaPackage v2 (not listed); LocalStack Pro coverage beyond the coverage
  page.

[rfc9725]: https://datatracker.ietf.org/doc/html/rfc9725
[ml-inputs]: https://docs.aws.amazon.com/medialive/latest/ug/inputs-supported-types.html
[ml-formats]: https://docs.aws.amazon.com/medialive/latest/ug/inputs-supported-formats.html
[cm-srt]: https://dev.classmethod.jp/articles/aws-elemental-medialive-srt-caller-input-type
[ml-price]: https://aws.amazon.com/medialive/pricing/
[mp-price]: https://aws.amazon.com/mediapackage/pricing/
[ls-cov]: https://docs.localstack.cloud/references/coverage/
[ls-road]: https://blog.localstack.cloud/the-road-ahead-for-localstack/
[moto-ml]: https://docs.getmoto.org/en/latest/docs/services/medialive.html
[moto-mp]: https://docs.getmoto.org/en/latest/docs/services/mediapackage.html
[moto-ivs]: https://docs.getmoto.org/en/latest/docs/services/ivs.html
[ivs-whip]: https://docs.aws.amazon.com/ivs/latest/RealTimeUserGuide/obs-whip-support.html
[ivs-rtmp]: https://docs.aws.amazon.com/ivs/latest/RealTimeUserGuide/rt-rtmp-publishing.html
[ivs-ssc]: https://docs.aws.amazon.com/ivs/latest/RealTimeUserGuide/ssc-overview.html
[ivs-quotas]: https://docs.aws.amazon.com/ivs/latest/RealTimeUserGuide/service-quotas.html
[ivs-rec]: https://docs.aws.amazon.com/ivs/latest/RealTimeUserGuide/rt-composite-recording.html
[ivs-priv]: https://docs.aws.amazon.com/ivs/latest/LowLatencyUserGuide/private-channels.html
[ivs-whep-undoc]: https://dev.to/aws/live-streaming-from-unity-real-time-playback-part-7-4mm2
[ivs-price]: https://aws.amazon.com/ivs/pricing/
[lk-price]: https://livekit.com/pricing
[lk-io]: https://docs.livekit.io/transport/media/ingress-egress/
[lk-out]: https://docs.livekit.io/transport/media/ingress-egress/egress/outputs/
[lk-comp]: https://docs.livekit.io/transport/media/ingress-egress/egress/composite-recording/
[lk-api]: https://docs.livekit.io/reference/other/egress/api/
[lk-hooks]: https://docs.livekit.io/home/server/webhooks/
[lk-egress]: https://github.com/livekit/egress
[ov-whip]: https://openvidu.io/blog/2026/09/08/low-latency-whip-ingestion/
[jibri]: https://github.com/jitsi/jibri
[janus-rec]: https://janus.conf.meetecho.com/docs/recordings.html
[janus-ab]: https://janus.conf.meetecho.com/docs/audiobridge.html
[ms-faq]: https://mediasoup.org/faq/
[ms-demos]: https://github.com/Kurento/mediasoup-demos
[pion-ex]: https://github.com/pion/example-webrtc-applications
[gst-comp]: https://gstreamer.freedesktop.org/documentation/compositor/index.html
[gst-whep]: https://gstreamer.freedesktop.org/documentation/webrtchttp/whepsrc.html
[gst-rswebrtc]: https://gstreamer.freedesktop.org/documentation/rswebrtc/whepclientsrc.html
[gst-whip]: https://gstreamer.freedesktop.org/documentation/rswebrtc/whipclientsink.html
[gst-rtmp2]: https://gstreamer.freedesktop.org/documentation/rtmp2/rtmp2sink.html
[gst-srt]: https://gstreamer.freedesktop.org/documentation/srt/srtsink.html
[gst-hls]: https://gstreamer.freedesktop.org/documentation/hls/hlssink2.html
[gst-dash]: https://gstreamer.freedesktop.org/documentation/dash/dashsink.html
[gst-valve]: https://gstreamer.freedesktop.org/documentation/coreelements/valve.html
[gst-lic]: https://gstreamer.freedesktop.org/documentation/frequently-asked-questions/licensing.html
[ff-filters]: https://ffmpeg.org/ffmpeg-filters.html
[ff-formats]: https://ffmpeg.org/ffmpeg-formats.html
[ff8]: https://lwn.net/Articles/1034813
[janus-gh]: https://github.com/meetecho/janus-gateway
[ms-gh]: https://github.com/versatica/mediasoup
[pion-gh]: https://github.com/pion/webrtc
[ff-legal]: https://ffmpeg.org/legal.html
[mtx-readme]: https://github.com/bluenviron/mediamtx
[mtx-webrtc]: https://github.com/bluenviron/mediamtx/blob/main/docs/4-read/03-webrtc.md
[mtx-hls]: https://github.com/bluenviron/mediamtx/blob/main/docs/4-read/06-hls.md
[mtx-auth]: https://mediamtx.org/docs/features/authentication
