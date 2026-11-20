# jmap-vtc-ffmpeg

A free-tools media backend for `jmap-vtc-server` livestreams and
recordings. Each hosted `VTCLivestream` is an ffmpeg process writing HLS
into a directory that any static web server serves; each push
`VTCLivestream` is an ffmpeg process sending to the moderator's
`streamUri`; each `VTCRecording` is a series of ffmpeg processes, one per
unpaused interval, joined into one MP4 when it stops. No vendor, no
account, no DRM. It is the vendor-neutral reference backend and the one
the VTC media hooks are tested against (decision 57).

## What it is

A support crate (workspace AGENTS.md §Support crates): it starts
processes and writes files.

| Item | What it does | Reference |
|---|---|---|
| `FfmpegMedia` | `start_livestream` (hosted: HLS into `{hls_root}/{id}/`; push: to `streamUri`), `stop_livestream` (graceful `q`, kill after the grace), `watch_livestream` (playlist or progress file → `MediaReporter` live; exit, timeout → failed), `playback_endpoint` (HLS under `base_url`); `start_recording`, `pause_recording`, `resume_recording` (one Matroska part per unpaused interval), `stop_recording` (concat → `RecordedFile`), `watch_recording` (a part that dies → `recording_stopped`) | draft-atwood-jmap-vtc-00 §Livestream States, §Pausing, Resuming, and Stopping a Recording |
| `FfmpegConfig` | the program, HLS root and base URL, private work root, `Encoding`, `HlsSettings`, start timeout, stop grace, poll interval | |
| `RecordedFile` | path, size, duration, `video/mp4`; `recording_file(blob_id)` for `recording_available` | |
| `args` | pure argument-vector builders: `hls_args`, `push_args`, `record_args`, `concat_args`, `concat_list`, `input_args`, `encode_args`; `Source` (`TestPattern` = `-f lavfi -i testsrc2` + `sine`, or any `InputSource`); `PushTarget` (stream key presentation); `PUSH_SCHEMES` | [ffmpeg], [ffmpeg formats], [ffmpeg filters] |
| `process` | `ProcessRunner` / `MediaProcess` (the seam tests fake), `TokioRunner` / `TokioProcess` (tokio, no shell, `kill_on_drop`), `ProcessExit` | |
| `FfmpegError` | `failure_reason()` (`rejected` / `unavailable`) and `set_error()` (`invalidProperties` naming `streamUri` or `mode`) | |

What the watchers report:

| Observed | VTC |
|---|---|
| hosted: the playlist lists a segment | `"live"` with the HLS endpoint, `latencyHint` `"standard"` |
| push: the `-progress` file shows output bytes | `"live"`, no endpoints |
| not live within the start timeout | process killed; `"failed"`, `"timeout"` |
| process ends before live | `"failed"`, `"rejected"` |
| process ends while live | `"failed"`, `"interrupted"` |
| a recording part ends on its own | recording `"stopped"` (its stop effect then joins the parts) |
| spawn fails (no ffmpeg) | `FfmpegError::Spawn` → `"unavailable"` |

Push destinations, per §Presenting the Stream Key:

| `streamUri` | ffmpeg | `streamKey` |
|---|---|---|
| `rtmp://`, `rtmps://` | `-f flv` | appended to the application path |
| `srt://` | `-f mpegts` | `streamid` query parameter, unless the URI has one |
| `https://` (WHIP, RFC 9725) | `-f whip` | `-authorization` bearer token |

## What it's for

A deployment that wants livestreams and recordings without a cloud media
service, a development setup, and CI: with `Source::TestPattern` the whole
media path runs from ffmpeg's synthetic source with no SFU, camera or
network. docs/design/vtc-broadcast-research.md §3.3 sketches it as the
offline CI oracle for the hooks; `jmap-vtc-aws` is the cloud sibling with
the same shape. It builds on `jmap-vtc-server`'s
`VtcBackend::run_media_effect` hook and `media_report::MediaReporter`. No
`*-types` or `*-server` crate depends on it.

## How to use

```toml
[dependencies]
jmap-vtc-ffmpeg = "0.6"
```

```rust,ignore
use std::sync::Arc;
use jmap_vtc_ffmpeg::{FfmpegConfig, FfmpegMedia, args::Source, url::Url};
use jmap_vtc_server::{MediaEffect, media_report::MediaReporter};

let media = Arc::new(FfmpegMedia::new(FfmpegConfig::new(
    "/srv/hls",                                        // served as-is by nginx, caddy, ServeDir
    Url::parse("https://media.example.com/hls/")?,
    "/var/lib/vtc",                                    // private: recordings, progress files
)));

// In your VtcBackend::run_media_effect (after commit under an atomic scope):
let reporter = MediaReporter::new(self, &system_caller, account_id.clone());
match effect {
    MediaEffect::StartLivestream(ls) => {
        let source = Source::input(call_feed_url(&ls.call_id));  // your compositor's output
        match media.start_livestream(&ls, &source).await {
            Ok(()) => spawn_watcher(&media, &ls.id),           // runs media.watch_livestream(&reporter, &id)
            Err(e) => { let _ = reporter.livestream_failed(&ls.id, e.failure_reason()).await; }
        }
    }
    MediaEffect::StopLivestream(ls) => { let _ = media.stop_livestream(&ls.id).await; }
    MediaEffect::StartRecording(r) => {
        let source = Source::input(call_feed_url(&r.call_id));
        if media.start_recording(&r, &source).await.is_ok() {
            spawn_recording_watcher(&media, &r.id);            // media.watch_recording(&reporter, &id)
        }
    }
    MediaEffect::PauseRecording(r) => { let _ = media.pause_recording(&r.id).await; }
    MediaEffect::ResumeRecording(r) => { let _ = media.resume_recording(&r.id).await; }
    MediaEffect::StopRecording(r) => {
        let _ = reporter.recording_processing(&r.id, None).await;
        match media.stop_recording(&r.id).await {
            Ok(file) => {
                let blob_id = store_blob(&file.path).await;    // your blob store
                let _ = reporter.recording_available(&r.id, file.recording_file(blob_id)).await;
            }
            Err(_) => { let _ = reporter.recording_failed(&r.id).await; }
        }
    }
    _ => {}                                                    // layout changes: see Gotchas
}
```

Advertise what it does: `livestreamModes: ["hosted", "push"]`,
`livestreamPushSchemes` from `args::PUSH_SCHEMES` (drop `"https"` if your
ffmpeg has no WHIP muxer), `supportsRecordingPause: true`, `layouts: []`.

## How it works

`FfmpegMedia` keeps a table of running processes keyed by livestream and
recording id; unlike `jmap-vtc-aws`, which is stateless, it owns live
child processes, so share one instance (in an `Arc`) between the backend
and the watcher tasks. Every process runs in the object's own directory,
and every output file is named relative to it, so configured paths never
reach ffmpeg's argument list (where `%` would be a pattern). Ids must be
RFC 8620 `Id`s (`A-Za-z0-9-_`), which cannot leave their root.

The watchers poll (every `poll_interval`, 250 ms by default): the process
with `try_wait`, then the readiness file's last 4 KiB. The adapter never
writes the JMAP store itself: state changes go through `MediaReporter`,
which checks the source state atomically with the write, so a report that
races a moderator's stop is answered `ReportError::WrongState` and changes
nothing. A failure report requests the stop effect, whose
`stop_livestream` finds the process already gone.

A stop writes `q` to ffmpeg's stdin, so it finishes its outputs (the HLS
`EXT-X-ENDLIST`, the Matroska index), waits `stop_grace` (10 s), then
kills. A recording pause finishes the current part; resume starts the
next; stop joins the parts with the concat demuxer (`-c copy`, no
re-encode, `+faststart`). `duration` is the wall-clock time the parts ran.

The `ProcessRunner` / `MediaProcess` traits are the only way the adapter
touches processes; the tests use a fake runner that writes the files
ffmpeg would write. Every public type that can grow is `#[non_exhaustive]`
and built with `Default` plus `with_*` methods.

## Gotchas

- **No compositor.** Like `jmap-vtc-aws`, the crate takes one feed per
  call (`Source::Input`: your compositor's output, an SDP file for an
  SFU's RTP forward, an SRT listener). `layout` and the
  `Change*Layout` effects are the compositor's; advertise `layouts: []`
  unless it draws them.
- **Serving is yours.** Point a static web server at `hls_root` with
  `base_url` as its URL. Do not serve `work_root`. HLS files stay after a
  stop (the playlist then ends with `EXT-X-ENDLIST`); delete
  `{hls_root}/{id}/` when you no longer need them, and a recording's
  directory once its file is stored.
- **Push destinations are the moderator's.** `streamUri` is limited to
  `rtmp`, `rtmps`, `srt` and `https`, so ffmpeg never writes a local file,
  but a moderator can still name an internal host. Apply your own address
  policy before `start_livestream` if that matters.
- **The stream key is on the command line.** ffmpeg takes it in its
  arguments, which other local users can read in `/proc`. Run the media
  host single-tenant. `PushTarget`'s `Debug` redacts it; do not log the
  argument vectors.
- **WHIP needs ffmpeg 8.0** (the `whip` muxer). Older builds fail the
  push, reported `"rejected"`.
- **Liveness is file-based.** Hosted goes live when the playlist lists a
  segment; push when the `-progress` file shows output bytes, which can be
  a moment before the remote end accepts the stream. A push that is
  refused after that point is reported `"interrupted"`, not `"rejected"`.
- **Not covered:** DASH, `timeReference` (the playlist carries
  `EXT-X-PROGRAM-DATE-TIME`, but the endpoint does not map it), caption
  and alternate-language tracks, per-viewer playback authorization (the
  default `playback_endpoints_for` serves the stored list), the idle stop,
  GStreamer, restart supervision, and processes surviving a restart of
  the host process (`kill_on_drop`).
- **Tests do not run ffmpeg.** `cargo test` uses the fake runner. The
  real-process tests need the `real-ffmpeg` feature and `--ignored`, and
  ffmpeg with libx264 on `PATH`:
  `cargo test -p jmap-vtc-ffmpeg --features real-ffmpeg -- --ignored`.

## References

- [draft-atwood-jmap-vtc-00] — JMAP VTC (§VTCLivestream, §Livestream States, §Presenting the Stream Key, §VTCRecording, §Pausing, Resuming, and Stopping a Recording)
- [RFC 8216] — HTTP Live Streaming
- [RFC 9725] — WebRTC-HTTP Ingestion Protocol (WHIP)
- [ffmpeg] — ffmpeg command-line tool
- [ffmpeg formats] — `hls`, `concat`, `matroska`, `mp4`, `flv`, `mpegts`, `whip`
- [ffmpeg filters] — `testsrc2`, `sine`
- docs/design/vtc-broadcast-research.md §3.3 — the free-tools backend sketch

[draft-atwood-jmap-vtc-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 8216]: https://www.rfc-editor.org/rfc/rfc8216
[RFC 9725]: https://www.rfc-editor.org/rfc/rfc9725
[ffmpeg]: https://ffmpeg.org/ffmpeg.html
[ffmpeg formats]: https://ffmpeg.org/ffmpeg-formats.html
[ffmpeg filters]: https://ffmpeg.org/ffmpeg-filters.html
