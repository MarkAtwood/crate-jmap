# jmap-vtc-aws

An AWS media backend for `jmap-vtc-server` hosted livestreams. Each hosted
`VTCLivestream` gets its own Amazon IVS low-latency channel: the call's
composited feed is pushed to the channel over RTMPS, viewers play its HLS
output, and IVS stream events move the livestream to `"live"` or
`"failed"`. Requests are signed with AWS Signature Version 4 by the crate
itself; there is no AWS SDK dependency.

## What it is

A support crate (workspace AGENTS.md §Support crates): it does network I/O
against AWS and holds AWS credentials and playback keys.

| Item | What it does | AWS reference |
|---|---|---|
| `Credentials` | access key id, secret key, optional STS session token; `Debug` redacts the secrets | [SigV4 signing][sigv4] |
| `ivs::IvsClient` | `create_channel`, `create_stream_key`, `get_stream_key`, `get_stream` (`None` when offline), `stop_stream` (`false` when offline), `delete_channel` | [IVS API reference][ivs-api] |
| `ivs::IvsLivestreams` | `start` (CreateChannel, named `jmap-vtc-{id}`, tagged with the livestream and call ids) → `ivs::IvsChannel`; `stop` (StopStream + DeleteChannel, `StopOutcome::DeletePending` on a 409 for a still-live channel); `playback_endpoint` (HLS `VTCPlaybackEndpoint`); `endpoints_for` (per-viewer token on private channels); `report_event` (IVS events → `MediaReporter`) | [CreateChannel][create], [DeleteChannel][delete], [StopStream][stop] |
| `ivs::IvsChannel` | the channel of one livestream: ARN, playback URL, RTMPS ingest (`rtmps_url`, `stream_key`), private or not; `Serialize` so the deployment stores it; `Debug` redacts the stream key | [Channel][channel] |
| `ivs::PlaybackKey`, `ivs::PlaybackClaims` | ES384 viewer tokens for private channels (`aws:channel-arn`, origin, viewer id, single use, maximum resolution, `exp`) | [playback tokens][tokens] |
| `ivs::IvsEvent` | parses EventBridge "IVS Stream State Change", "IVS Stream Health Change" and "IVS Limit Breach" events | [EventBridge events][events] |
| `AwsError`, `AwsErrorCode` | typed AWS errors; `failure_reason()` gives the `VTCLivestream.failureReason`, `set_error()` the RFC 8620 SetError | [IVS errors][errors] |

Error and event mapping:

| AWS | VTC |
|---|---|
| `ValidationException`, `ConflictException`, push mode | `failureReason` `"rejected"` |
| `ServiceQuotaExceededException`, throttling, access and account errors, 5xx, transport | `failureReason` `"unavailable"` |
| event Stream Start | `"live"` with the HLS endpoint |
| event Stream End | `"failed"`, `"interrupted"` (`WrongState` if the server already stopped it) |
| event Stream Failure, Limit Breach: Concurrent Broadcasts | `"failed"`, `"unavailable"` |
| event Limit Breach: Ingest Bitrate or Ingest Resolution | `"failed"`, `"rejected"` |
| `ServiceQuotaExceededException` / `ThrottlingException` / `AccessDeniedException` / `ResourceNotFoundException` / `ValidationException` | SetError `overQuota` / `rateLimit` / `forbidden` / `notFound` / `invalidProperties` |

## What it's for

A deployment whose calls run on its own SFU and that wants AWS to serve
hosted livestreams to a large audience. docs/design/vtc-broadcast-research.md
(Recommendation 4) picks this path as the least to build: a compositor
turns the call into one feed and pushes it over RTMPS to IVS, and IVS
does the transcoding, the CDN and playback authorization. The crate builds
on `jmap-vtc-server`'s `VtcBackend::run_media_effect` hook and its
`media_report::MediaReporter`. No `*-types` or `*-server` crate depends
on it.

## How to use

```toml
[dependencies]
jmap-vtc-aws = "0.5"
```

```rust,ignore
use jmap_vtc_aws::{Credentials, ivs::{ChannelTemplate, IvsClient, IvsEvent, IvsLivestreams}};
use jmap_vtc_server::{MediaEffect, media_report::MediaReporter};

let ivs = IvsLivestreams::new(
    IvsClient::new(Credentials::new(key_id, secret), "us-west-2")?,
    ChannelTemplate::default(),
);

// In your VtcBackend::run_media_effect (after commit under an atomic scope):
match effect {
    MediaEffect::StartLivestream(ls) => match ivs.start(&ls).await {
        Ok(channel) => {
            store_channel(&ls.id, &channel);                    // your storage
            compositor.push(&ls.call_id, channel.rtmps_url(), channel.stream_key());
        }
        Err(e) => {
            let reporter = MediaReporter::new(self, &system_caller, account_id.clone());
            let _ = reporter.livestream_failed(&ls.id, e.failure_reason()).await;
        }
    },
    MediaEffect::StopLivestream(ls) => {
        if let Some(channel) = load_channel(&ls.id) {
            compositor.stop(&ls.call_id);
            let _ = ivs.stop(&channel).await;                   // DeletePending: retry on Stream End
        }
    }
    _ => {}
}

// Your EventBridge consumer (an SQS queue, a Lambda, an HTTPS target):
if let Some(event) = IvsEvent::from_eventbridge(&json) {
    if let Some(channel) = event.channel_arn.as_deref().and_then(channel_by_arn) {
        let reporter = MediaReporter::new(&backend, &system_caller, account_id);
        let _ = ivs.report_event(&reporter, &channel, &event).await;
    }
}

// In VtcBackend::playback_endpoints_for:
ivs.endpoints_for(&channel, stored, Some(&viewer_id), SystemTime::now())
```

## How it works

`IvsClient` posts JSON to `https://ivs.{region}.amazonaws.com/{Action}`
and signs each request with SigV4 (`host`, `content-type`, `x-amz-date`
and, for temporary credentials, `x-amz-security-token`; HMAC-SHA256 from
`ring`). A non-2xx answer becomes `AwsError::Service` with the code from
`x-amzn-ErrorType` (or the body's `__type`) and the `exceptionMessage`.
The signer and the JSON client are service-neutral and private; a
MediaLive and MediaPackage module would sit next to `ivs` and reuse them
with its own service name and paths.

`IvsLivestreams` keeps no per-livestream state. `start` returns an
`IvsChannel`; the deployment stores it with the livestream and passes it
back to `stop`, `endpoints_for` and `report_event`. The adapter never
writes the JMAP store itself: state changes go through `MediaReporter`,
which checks the livestream's state atomically with the write, so a late or
duplicate IVS event is answered `ReportError::WrongState` and changes
nothing.

A private channel (`with_private_playback`) is created with
`authorized: true`. Each viewer's endpoint URL then carries `?token=` with
an ES384 JWT signed by the deployment's playback key, valid for the
configured lifetime (5 minutes by default), with `expiresAt` set and
`credential` `null`, as draft-atwood-jmap-vtc-00 §Playback Authorization
allows for protocols whose players take the authorization in the URL.
Every public type that can grow is `#[non_exhaustive]`; `CreateChannelInput`
is a plain struct built with `..Default::default()`.

## Gotchas

- **Hosted mode only.** `start` refuses a push livestream with
  `AwsError::UnsupportedMode` (`"rejected"`): IVS cannot push to the
  moderator's `streamUri`, so the compositor pushes there directly.
- **No compositor.** The crate provisions the channel and hands back the
  RTMPS URL and stream key. Turning the call into one feed (LiveKit Egress,
  GStreamer) is the deployment's (research §2).
- **Recordings are not covered.** IVS recording to S3 writes HLS segments,
  not the one file a `VTCRecording` blob needs, and IVS has no pause; a
  remux step and the recording effects are left for later.
- **Not covered:** IVS Real-Time stages and server-side composition,
  MediaLive and MediaPackage (DRM, DASH, DVR windows), CloudFront signed
  URLs, `timeReference`, caption tracks, credential discovery (environment,
  profiles, instance metadata, STS refresh) and automatic retries. A
  throttled call fails with `ThrottlingException`; retry it yourself.
- **Events are best effort.** IVS sends EventBridge events that may be
  missing, hours late or out of order. A Stream End that arrives before
  its Stream Start fails a starting livestream as `"interrupted"`. If no
  event arrives, the livestream stays `"starting"` until you report a
  timeout (`livestream_failed` with `"timeout"`) or poll `get_stream`.
- **Deleting a live channel is a 409.** `stop` stops the stream and tries
  `DeleteChannel`; on `StopOutcome::DeletePending` call `stop` again after
  the channel's Stream End event, or the channel lingers (and is billed
  while it receives input).
- **Playback keys are PKCS #8 P-384.** Convert an OpenSSL `EC PRIVATE KEY`
  with `openssl pkcs8 -topk8 -nocrypt`, and import the public half into
  IVS (`ImportPlaybackKeyPair`) yourself. With a viewer id or single-use
  token IVS caps the token lifetime at 10 minutes.
- **Tests never call AWS.** They run against a wiremock stub with the AWS
  documentation's example keys. Nothing here has been run against a real
  account.

## References

- [draft-atwood-jmap-vtc-00] — JMAP VTC (§VTCLivestream, §Livestream States, §Playback Authorization)
- [Amazon IVS Low-Latency Streaming API Reference][ivs-api]
- [Using Amazon EventBridge with IVS Low-Latency Streaming][events]
- [Generate and Sign IVS Playback Tokens][tokens]
- [Create a signed AWS API request][sigv4] — Signature Version 4
- docs/design/vtc-broadcast-research.md — the media backend research

[draft-atwood-jmap-vtc-00]: https://github.com/MarkAtwood/jmap-chat-spec
[ivs-api]: https://docs.aws.amazon.com/ivs/latest/LowLatencyAPIReference/Welcome.html
[create]: https://docs.aws.amazon.com/ivs/latest/LowLatencyAPIReference/API_CreateChannel.html
[delete]: https://docs.aws.amazon.com/ivs/latest/LowLatencyAPIReference/API_DeleteChannel.html
[stop]: https://docs.aws.amazon.com/ivs/latest/LowLatencyAPIReference/API_StopStream.html
[channel]: https://docs.aws.amazon.com/ivs/latest/LowLatencyAPIReference/API_Channel.html
[errors]: https://docs.aws.amazon.com/ivs/latest/LowLatencyAPIReference/CommonErrors.html
[events]: https://docs.aws.amazon.com/ivs/latest/LowLatencyUserGuide/eventbridge.html
[tokens]: https://docs.aws.amazon.com/ivs/latest/LowLatencyUserGuide/private-channels-generate-tokens.html
[sigv4]: https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_sigv-create-signed-request.html
