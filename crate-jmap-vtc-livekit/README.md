# jmap-vtc-livekit

A LiveKit media backend for `jmap-vtc-server` livestreams and recordings.
Each `VTCLivestream` and each `VTCRecording` of a call held in a LiveKit
room is one LiveKit room-composite egress: LiveKit renders the room in
the requested layout, encodes it, and streams it to the moderator's
`streamUri`, writes HLS for hosted playback, or uploads an MP4 to object
storage. Egress webhooks move the objects to `"live"`, `"failed"`,
`"stopped"` and `"processing"`. The crate talks to LiveKit over its
Twirp JSON API with access tokens it signs itself; there is no LiveKit
SDK dependency.

## What it is

A support crate (workspace AGENTS.md §Support crates): it does network I/O
against a LiveKit server and holds its API secret.

| Item | What it does | LiveKit reference |
|---|---|---|
| `LiveKitMedia` | `start_livestream`, `start_recording` (one room-composite egress each) → `EgressJob`; `stop` (`StopEgress`, idempotent); `change_layout` (`UpdateLayout`); `playback_endpoints` (hosted); `push_url` (§Presenting the Stream Key); `egress_outcome` (pure: egress state → VTC transition); `report_egress` (applies it through `MediaReporter`); `advertised_layouts`, `advertised_modes` | [Egress API][lk-api], [composite recording][lk-comp] |
| `LiveKitConfig` | storage (`S3Upload` or the egress server's), recording key prefix, `HostedOutput`, default layout, VTC → LiveKit layout table, encoding preset, custom template | [outputs][lk-out] |
| `HostedOutput::Hls` / `HlsOutput` | hosted livestreams as HLS segments with a live playlist in object storage, played from a base URL | [outputs][lk-out] |
| `HostedOutput::Origin` / `OriginOutput` | hosted livestreams streamed over RTMP or SRT into a media origin (MediaMTX for WHEP), with `{id}` endpoint templates | [MediaMTX][mtx] |
| `EgressJob`, `JobObject` | the egress serving one object; `Serialize`, so the deployment stores it | |
| `UploadedFile` | location, size, duration, `video/mp4` of a finished recording; `recording_file(blob_id)` for `recording_available` | `FileInfo` |
| `egress::EgressClient` | `start_egress` (the unified `StartEgress` RPC; needs LiveKit server v1.13.2 and egress v1.14.0, see Gotchas), `update_layout`, `stop_egress`, `list_egress`, over `POST /twirp/livekit.Egress/{Method}` | [Egress API][lk-api], [Twirp][twirp], [twirp.go][proto-twirp] |
| `egress::*` | `StartEgressRequest` (room composite: `TemplateSource`), `Output` (`OutputConfig` plus per-output storage), `FileOutput`, `SegmentOutput`, `StreamOutput`, `S3Upload` (redacted `Debug`), `EgressInfo`, `EgressStatus`, `FileInfo`, `SegmentsInfo`, `StreamInfo` | [livekit_egress.proto][proto-egress] |
| `token::ApiKey`, `Claims`, `VideoGrant` | HS256 access tokens: `sign`, `verify` (issuer, `exp`, `nbf`, one minute of leeway); the secret never reaches `Debug` | [tokens and grants][lk-tokens], [auth][proto-auth] |
| `webhook::WebhookReceiver`, `WebhookEvent` | verify a webhook's `Authorization` token and its body `sha256`, then parse the event | [webhooks][lk-hooks], [webhook package][proto-webhook] |
| `LiveKitError`, `TwirpCode` | `failure_reason()` (`rejected` / `unavailable`) and `set_error()` | [Twirp errors][twirp] |

Egress state to VTC transition (`egress_outcome`):

| `EgressStatus` | Livestream | Recording |
|---|---|---|
| `EGRESS_ACTIVE` | `"live"` (endpoints when hosted, none in push mode) | nothing |
| `EGRESS_COMPLETE` | `"stopped"`: the room emptied, the idle stop (after a moderator's stop: `WrongState`, nothing changes) | `"stopped"` if still running, then `"processing"` with the duration, and the file to import |
| `EGRESS_LIMIT_REACHED` | `"failed"`, `"interrupted"` | as `EGRESS_COMPLETE` (the file so far) |
| `EGRESS_FAILED` | `"failed"`: `"interrupted"` if it had started, else `"rejected"` | `"failed"` |
| `EGRESS_ABORTED` | `"failed"`, `"unavailable"` | `"failed"` |
| `EGRESS_STARTING`, `EGRESS_ENDING` | nothing | nothing |

Push destinations, per §Presenting the Stream Key:

| `streamUri` | LiveKit protocol | `streamKey` |
|---|---|---|
| `rtmp://`, `rtmps://` | `RTMP` | appended to the application path |
| `srt://` | `SRT` | `streamid` query parameter, unless the URI has one |

## What it's for

A deployment whose calls run on a LiveKit SFU. docs/design/vtc-broadcast-research.md
§2.2 says that with LiveKit the compositor is LiveKit Egress, used as is,
and §3.2 sketches this backend (with MediaMTX for WHEP). It builds on
`jmap-vtc-server`'s `VtcBackend::run_media_effect` hook and
`media_report::MediaReporter`, like its siblings `jmap-vtc-aws` (IVS,
cloud) and `jmap-vtc-ffmpeg` (free tools). No `*-types` or `*-server`
crate depends on it.

## How to use

```toml
[dependencies]
jmap-vtc-livekit = "0.6"
```

```rust,ignore
use std::time::SystemTime;
use jmap_vtc_livekit::{HlsOutput, HostedOutput, LiveKitConfig, LiveKitMedia, Reported};
use jmap_vtc_livekit::egress::{EgressClient, S3Upload};
use jmap_vtc_livekit::token::ApiKey;
use jmap_vtc_livekit::webhook::WebhookReceiver;
use jmap_vtc_livekit::url::Url;
use jmap_vtc_server::{MediaEffect, media_report::MediaReporter};

let key = ApiKey::new(api_key, api_secret);
let media = LiveKitMedia::new(
    EgressClient::new(&Url::parse("wss://livekit.example.com")?, key.clone())?,
    LiveKitConfig::default()
        .with_storage(S3Upload::new(s3_key, s3_secret, "media").with_region("us-west-2"))
        .with_hosted(HostedOutput::Hls(HlsOutput::new(
            "vtc/live/",
            Url::parse("https://cdn.example.com/vtc/live/")?,  // public URL of that prefix
        ))),
);
let webhooks = WebhookReceiver::new(key);

// In your VtcBackend::run_media_effect (after commit under an atomic scope):
let reporter = MediaReporter::new(self, &system_caller, account_id.clone());
match effect {
    MediaEffect::StartLivestream(ls) => match media.start_livestream(&ls, &room_of(&ls.call_id)).await {
        Ok(job) => store_job(&job),                              // your storage, keyed by job.egress_id
        Err(e) => { let _ = reporter.livestream_failed(&ls.id, e.failure_reason()).await; }
    },
    MediaEffect::StartRecording(r) => match media.start_recording(&r, &room_of(&r.call_id)).await {
        Ok(job) => store_job(&job),
        Err(_) => { let _ = reporter.recording_stopped(&r.id).await; /* then recording_failed */ }
    },
    MediaEffect::StopLivestream(ls) => if let Some(job) = job_for(&ls.id) { let _ = media.stop(&job).await; },
    MediaEffect::StopRecording(r) => if let Some(job) = job_for(&r.id) { let _ = media.stop(&job).await; },
    MediaEffect::ChangeLivestreamLayout(ls) => if let Some(job) = job_for(&ls.id) {
        let _ = media.change_layout(&job, ls.layout.as_ref()).await;
    },
    MediaEffect::ChangeRecordingLayout(r) => if let Some(job) = job_for(&r.id) {
        let _ = media.change_layout(&job, r.layout.as_ref()).await;
    },
    _ => {}                                                      // no pause: see Gotchas
}

// Your webhook endpoint (Content-Type application/webhook+json):
let event = webhooks.receive(&body, &headers["authorization"], SystemTime::now())?;
if let Some(info) = event.egress_info {
    if let Some(job) = job_by_egress_id(&info.egress_id) {
        let reporter = MediaReporter::new(&backend, &system_caller, account_id);
        if let Ok(Reported::RecordingProcessing(file)) = media.report_egress(&reporter, &job, &info).await {
            match import_blob(&file.location).await {              // your blob store
                Ok(blob_id) => { let _ = reporter.recording_available(&job_id, file.recording_file(blob_id)).await; }
                Err(_) => { let _ = reporter.recording_failed(&job_id).await; }
            }
        }
    }
}
```

Advertise what it does: `livestreamModes` from `advertised_modes()`,
`livestreamPushSchemes` from `PUSH_SCHEMES` (`rtmp`, `rtmps`, `srt`),
`layouts` from `advertised_layouts()`, and `supportsRecordingPause: false`.

## How it works

`EgressClient` posts JSON to `{server}/twirp/livekit.Egress/{Method}`
with `Authorization: Bearer` and a fresh HS256 token (`iss` the API key,
`video.roomRecord: true`, ten minutes). A `wss://` server URL is mapped to
`https://`. Requests use the proto field names (`room_name`), which
LiveKit's protojson decoder accepts. LiveKit's Twirp server answers with
the proto names, every field present and `int64` values as strings;
webhook bodies use protojson's defaults instead (lowerCamelCase, default
values omitted, so `EGRESS_STARTING` is absent). The `egress` response
types read both. A non-2xx answer becomes `LiveKitError::Twirp` with the
body's `code` and `msg`.

Egresses start with LiveKit's unified `StartEgress` RPC (decision 64),
not `StartRoomCompositeEgress`, which the proto lists under "TODO:
deprecate". `UpdateLayout`, `StopEgress` and `ListEgress` are not
deprecated and stay. A room composite is a `StartEgressRequest` whose
`source` oneof is `template` (`TemplateSource`: `layout`,
`custom_base_url`); protojson writes a oneof as its member's field, so a
recording's request is

```json
{"room_name": "vtc-C1", "template": {"layout": "speaker"},
 "outputs": [{"file": {"file_type": "MP4", "filepath": "jmap-vtc/recordings/R1.mp4"},
              "storage": {"s3": {"access_key": "...", "secret": "...", "bucket": "media"}}}]}
```

Storage now sits on each `Output` (`storage`, "falls back to request,
then server") instead of inside the file or segment output; the crate
sets it per output, as it set `s3` per output before. The egress still
takes at most one file, one stream and one segmented output.

`LiveKitMedia` keeps no state. A start returns an `EgressJob`; the
deployment stores it and finds it by `egress_id` when a webhook arrives.
The adapter never writes the JMAP store itself: state changes go through
`MediaReporter`, which checks the source state atomically with the
write, so a late, duplicate or raced webhook is answered
`ReportError::WrongState` and changes nothing. That is also what hands a
recording's file out for import once: only the first `egress_ended` moves
the recording to `"processing"`.

Webhooks are verified the way LiveKit's own receiver does it: the
`Authorization` value is a JWT from the API key, valid for five minutes;
after the signature, issuer and expiry checks, its `sha256` claim must be
the base64 SHA-256 of the exact body bytes.

**Recording pause.** LiveKit has no egress pause RPC (the Egress API
lists start, update layout, update stream, list and stop). The crate
reports pause as unsupported rather than emulating it. Emulating would
mean stopping the egress on pause and starting a new one on resume:
each start launches a new headless browser and encoder (seconds of
startup lost at every resume), and the result is several MP4 files in
object storage that something must download and join while
`"processing"`. This crate does HTTP only and cannot join media;
`jmap-vtc-ffmpeg` joins its own local parts. The draft's
`supportsRecordingPause` exists for this case (research S1): with it
`false`, `VTCRecording/set` refuses `"paused"` and never sends the
pause effect.

Every public type that can grow is `#[non_exhaustive]` and built with a
constructor plus `with_*` methods.

## Gotchas

- **Rooms are yours.** The crate needs the LiveKit room name of each
  call and does not create, join or end rooms (no RoomService calls).
- **Hosted playback from object storage.** `HlsOutput::base_url` must be
  the public URL of `key_prefix` in the bucket (a CDN, or the bucket's
  website endpoint). The livestream goes `"live"` on `EGRESS_ACTIVE`,
  which can be a segment duration before the first segment is uploaded;
  players retry the playlist. Old segments are not deleted.
- **Sub-second playback is a deployment option.** `HostedOutput::Origin`
  pushes into MediaMTX (or another origin) and lists its WHEP and HLS
  URLs. The crate does not mint MediaMTX reader tokens: authorize viewers
  in `VtcBackend::playback_endpoints_for`, or leave the origin open.
- **Recordings land in object storage.** The deployment imports the file
  at `UploadedFile::location` into its blob store and reports
  `"available"`; until then the recording stays `"processing"`.
- **Push destinations are the moderator's.** `streamUri` is limited to
  `rtmp`, `rtmps` and `srt`, but a moderator can still name an internal
  host that the egress workers can reach. Apply your own address policy
  before `start_livestream` if that matters. The stream key travels in
  the request body to LiveKit; `StreamOutput`'s `Debug` redacts the URLs.
- **No WHIP push.** LiveKit Egress streams RTMP and SRT only.
- **Webhooks are best effort.** LiveKit retries but does not guarantee
  delivery. Without the final webhook an object stays where it is; poll
  `EgressClient::list_egress` and feed the infos to `report_egress`.
- **Egress per type.** "Egress supports only a single item per type"
  (the egress worker refuses "multiple file outputs", and likewise for
  stream and segmented outputs), so a livestream is one egress and a
  recording another, even for the same call (each costs a browser and an
  encoder on the egress workers).
- **Needs LiveKit server v1.13.2 and egress v1.14.0 or later.** The crate
  starts egresses with the unified `StartEgress` RPC (decision 64), and
  older LiveKit refuses it; there is no fallback to
  `StartRoomCompositeEgress`. The first server release that serves it is
  [v1.13.2][lk-1.13.2] (2026-06-27, "egress v2 api", [livekit#4592][lk-4592]).
  Through [v1.12.0][lk-1.12.0] the Twirp router has no such method and
  answers `bad_route` (404); [v1.13.1][lk-1.13.1] has the method but
  answers "not implemented" (a plain Go error, so `internal`, 500). Both
  become `LiveKitError::Twirp` with `failure_reason()` `unavailable`. The
  egress workers must be [v1.14.0][eg-1.14.0] (2026-08-06) or later too:
  earlier workers have no case for the new request kind and refuse it
  ("request has missing or invalid field: request", psrpc
  `InvalidArgument`), which presumably reaches the caller as
  `invalid_argument` (so `rejected`, though the moderator did nothing
  wrong). A self-hosted deployment upgrades both before using this
  crate.
- **Ed25519 tokens.** LiveKit can also sign with Ed25519 keys
  (`livekit-v2+jwt`); this crate verifies HS256 only.
- **Unconfirmed** (checked against the docs, livekit/protocol `main`,
  and the livekit/livekit and livekit/egress sources at the tags named
  above on 2026-11-21, not against a running server): that
  `EgressInfo.started_at` is set only once the egress is active (used to
  tell `"interrupted"` from `"rejected"`); which Twirp code `StartEgress`
  returns for a room that does not exist; the exact Twirp codes older
  servers and workers answer `StartEgress` with (above); LiveKit's
  default layout when `layout` is empty (the crate always sends one);
  egress latency.
- **Tests never call LiveKit.** They run against a wiremock stub, and the
  token and webhook signatures are checked against values computed outside
  the crate (Python `hmac`/`hashlib`, `openssl dgst`). Nothing here has
  been run against a real LiveKit server.

## References

- [draft-atwood-jmap-vtc-00] — JMAP VTC (§VTCRecording, §VTCLivestream, §Livestream States, §Presenting the Stream Key, §Composition Layouts, §Pausing, Resuming, and Stopping a Recording)
- [LiveKit Egress API][lk-api]
- [LiveKit Egress outputs][lk-out]
- [LiveKit composite recording][lk-comp]
- [LiveKit tokens and grants][lk-tokens]
- [LiveKit webhooks][lk-hooks]
- [livekit/protocol `livekit_egress.proto`][proto-egress] (`StartEgress`, `StartEgressRequest`, `TemplateSource`, `Output`, `StorageConfig`), [`livekit_egress.twirp.go`][proto-twirp] (route `/twirp/livekit.Egress/StartEgress`; `DiscardUnknown: true`; `UseProtoNames: !jsonCamelCase`, `EmitUnpopulated: !jsonSkipDefaults`), [`livekit_webhook.proto`][proto-webhook-proto], [`auth`][proto-auth], [`webhook`][proto-webhook]
- livekit/livekit: [v1.13.2][lk-1.13.2], the first release serving `StartEgress` ([egress v2 api, #4592][lk-4592]); [v1.13.1 `egress.go`][lk-1.13.1] (the "not implemented" stub); [v1.12.0 `egress.go`][lk-1.12.0] (no `StartEgress`)
- livekit/egress: [v1.14.0][eg-1.14.0], the first worker release that handles the request (`pkg/config/pipeline.go`, `pkg/config/output.go`)
- [Twirp wire protocol][twirp]
- [MediaMTX][mtx]
- docs/design/vtc-broadcast-research.md §2, §3.2 — the media backend research

[draft-atwood-jmap-vtc-00]: https://github.com/MarkAtwood/jmap-chat-spec
[lk-api]: https://docs.livekit.io/reference/other/egress/api/
[lk-out]: https://docs.livekit.io/transport/media/ingress-egress/egress/outputs/
[lk-comp]: https://docs.livekit.io/transport/media/ingress-egress/egress/composite-recording/
[lk-tokens]: https://docs.livekit.io/frontends/reference/tokens-grants/
[lk-hooks]: https://docs.livekit.io/home/server/webhooks/
[proto-egress]: https://github.com/livekit/protocol/blob/main/protobufs/livekit_egress.proto
[proto-twirp]: https://github.com/livekit/protocol/blob/main/livekit/livekit_egress.twirp.go
[lk-1.13.2]: https://github.com/livekit/livekit/releases/tag/v1.13.2
[lk-4592]: https://github.com/livekit/livekit/pull/4592
[lk-1.13.1]: https://github.com/livekit/livekit/blob/v1.13.1/pkg/service/egress.go
[lk-1.12.0]: https://github.com/livekit/livekit/blob/v1.12.0/pkg/service/egress.go
[eg-1.14.0]: https://github.com/livekit/egress/releases/tag/v1.14.0
[proto-webhook-proto]: https://github.com/livekit/protocol/blob/main/protobufs/livekit_webhook.proto
[proto-auth]: https://github.com/livekit/protocol/tree/main/auth
[proto-webhook]: https://github.com/livekit/protocol/tree/main/webhook
[twirp]: https://twitchtv.github.io/twirp/docs/spec_v7.html
[mtx]: https://github.com/bluenviron/mediamtx
