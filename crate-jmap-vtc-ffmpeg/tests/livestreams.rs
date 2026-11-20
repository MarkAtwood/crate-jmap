//! The livestream adapter end to end without ffmpeg: `jmap-vtc-server`
//! handlers and `MemoryBackend` (which records the media effects), the
//! adapter over a fake process runner, and the watcher reporting through
//! `MediaReporter`.
//!
//! Oracles: draft-atwood-jmap-vtc-00 §Livestream States (`"starting"` →
//! `"live"` → `"stopped"`, `"failed"` with `failureReason`), §VTCLivestream
//! (`playbackEndpoints` "Always empty in push mode", "empty once it is
//! `"stopped"` or `"failed"`"; the `failureReason` values "timeout",
//! "rejected", "unavailable", "interrupted"), §Presenting the Stream Key
//! (RTMP stream name appended to the application path); the ffmpeg
//! documentation for the `hls` and `flv` muxers.

mod common;

use std::future::Future;
use std::time::{Duration, Instant};

use common::{
    BASE_URL, FakeRunner, backend, config, last_effect, livestream_get, livestream_set,
    started_livestream, system,
};
use jmap_types::Id;
use jmap_vtc_ffmpeg::args::Source;
use jmap_vtc_ffmpeg::{FfmpegError, FfmpegMedia};
use jmap_vtc_server::MediaEffect;
use jmap_vtc_server::media_report::MediaReporter;
use jmap_vtc_server::memory::MemoryBackend;
use jmap_vtc_types::LivestreamFailureReason;
use serde_json::{Value, json};

const ACCOUNT: &str = common::ACCOUNT;

/// Poll `get` until `pred` holds, for up to 5 s.
async fn until<F, Fut>(get: F, pred: impl Fn(&Value) -> bool) -> Value
where
    F: Fn() -> Fut,
    Fut: Future<Output = Value>,
{
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let v = get().await;
        if pred(&v) {
            return v;
        }
        assert!(Instant::now() < deadline, "timed out; last value {v}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

async fn create(b: &MemoryBackend, create: Value) -> Id {
    let r = livestream_set(b, json!({"create": {"s1": create}})).await;
    Id::from(
        r["created"]["s1"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{r}")),
    )
}

/// Create (effect) → HLS process → playlist → `"live"` with the static
/// HLS endpoint → moderator stop (effect) → graceful stop; the watcher
/// then returns without a report.
#[tokio::test]
async fn hosted_livestream_lifecycle() {
    let runner = FakeRunner::new();
    let media = FfmpegMedia::with_runner(config(), runner.clone());
    let b = backend();
    let id = create(&b, json!({"callId": "C1", "mode": "hosted"})).await;
    media
        .start_livestream(&started_livestream(&b), &Source::test_pattern())
        .await
        .unwrap();

    let p = runner.last();
    assert_eq!(p.dir, media.config().hls_root.join(id.as_ref() as &str));
    assert_eq!(p.after("-i"), "testsrc2=size=1280x720:rate=30");
    assert_eq!(p.after("-f"), "lavfi");
    assert_eq!(p.args.last().unwrap(), "index.m3u8");
    assert!(p.args.iter().any(|a| a == "hls"));

    let caller = system();
    let reporter = MediaReporter::new(&b, &caller, Id::from(ACCOUNT));
    let driver = async {
        let ls = until(|| livestream_get(&b, &id), |v| v["state"] == "live").await;
        assert_eq!(ls["playbackEndpoints"][0]["protocol"], "hls");
        assert_eq!(
            ls["playbackEndpoints"][0]["url"],
            json!(format!("{BASE_URL}{id}/index.m3u8"))
        );
        assert_eq!(ls["playbackEndpoints"][0]["latencyHint"], "standard");
        assert!(ls["playbackEndpoints"][0]["credential"].is_null());

        let r = livestream_set(&b, json!({"update": {id.as_ref(): {"state": "stopped"}}})).await;
        assert!(r["notUpdated"].is_null(), "{r}");
        let Some(MediaEffect::StopLivestream(stopped)) = last_effect(&b) else {
            panic!("no stop effect");
        };
        media.stop_livestream(&stopped.id).await.unwrap();
    };
    let (watched, ()) = tokio::join!(media.watch_livestream(&reporter, &id), driver);
    watched.unwrap();

    {
        let ctl = p.ctl();
        assert!(ctl.stop_requested && !ctl.killed, "{ctl:?}");
    }
    let ls = livestream_get(&b, &id).await;
    assert_eq!(ls["state"], "stopped");
    assert_eq!(ls["playbackEndpoints"], json!([]));
    // A second stop (a duplicate effect) is fine.
    media.stop_livestream(&id).await.unwrap();
}

/// Push to RTMP: the key is appended to the application path, `"live"`
/// comes from the progress file and carries no endpoints, and a process
/// that dies while live is `"interrupted"`.
#[tokio::test]
async fn push_livestream_lost_while_live() {
    let runner = FakeRunner::new();
    let media = FfmpegMedia::with_runner(config(), runner.clone());
    let b = backend();
    let id = create(
        &b,
        json!({
            "callId": "C1",
            "streamUri": "rtmp://live.example.com/app",
            "streamKey": "sk_live_abc123def456"
        }),
    )
    .await;
    media
        .start_livestream(&started_livestream(&b), &Source::test_pattern())
        .await
        .unwrap();
    let p = runner.last();
    let n = p.args.len();
    assert_eq!(
        p.args[n - 3..],
        [
            "-f",
            "flv",
            "rtmp://live.example.com/app/sk_live_abc123def456"
        ]
    );
    assert_eq!(p.after("-progress"), "progress");
    assert_eq!(
        p.dir,
        media
            .config()
            .work_root
            .join("livestreams")
            .join(id.as_ref() as &str)
    );

    let caller = system();
    let reporter = MediaReporter::new(&b, &caller, Id::from(ACCOUNT));
    let driver = async {
        let ls = until(|| livestream_get(&b, &id), |v| v["state"] == "live").await;
        assert_eq!(ls["playbackEndpoints"], json!([]));
        p.exit(Some(1));
    };
    let (watched, ()) = tokio::join!(media.watch_livestream(&reporter, &id), driver);
    watched.unwrap();

    let ls = livestream_get(&b, &id).await;
    assert_eq!(ls["state"], "failed");
    assert_eq!(ls["failureReason"], "interrupted");
    assert!(matches!(
        last_effect(&b),
        Some(MediaEffect::StopLivestream(_))
    ));
    // The progress directory is gone, and the stop effect finds nothing.
    assert!(!p.dir.exists());
    media.stop_livestream(&id).await.unwrap();
}

/// A process that ends before any output (bad input, refused
/// destination) is `"rejected"`.
#[tokio::test]
async fn exit_before_live_is_rejected() {
    let runner = FakeRunner::new().silent();
    let media = FfmpegMedia::with_runner(config(), runner.clone());
    let b = backend();
    let id = create(&b, json!({"callId": "C1", "mode": "hosted"})).await;
    media
        .start_livestream(&started_livestream(&b), &Source::test_pattern())
        .await
        .unwrap();
    runner.last().exit(Some(1));

    let caller = system();
    let reporter = MediaReporter::new(&b, &caller, Id::from(ACCOUNT));
    media.watch_livestream(&reporter, &id).await.unwrap();
    let ls = livestream_get(&b, &id).await;
    assert_eq!(ls["state"], "failed");
    assert_eq!(ls["failureReason"], "rejected");
}

/// No playlist within the start timeout: the process is killed and the
/// livestream fails with `"timeout"`.
#[tokio::test]
async fn no_output_times_out() {
    let runner = FakeRunner::new().silent();
    let media = FfmpegMedia::with_runner(
        config().with_start_timeout(Duration::from_millis(50)),
        runner.clone(),
    );
    let b = backend();
    let id = create(&b, json!({"callId": "C1", "mode": "hosted"})).await;
    media
        .start_livestream(&started_livestream(&b), &Source::test_pattern())
        .await
        .unwrap();

    let caller = system();
    let reporter = MediaReporter::new(&b, &caller, Id::from(ACCOUNT));
    media.watch_livestream(&reporter, &id).await.unwrap();
    assert!(runner.last().ctl().killed);
    let ls = livestream_get(&b, &id).await;
    assert_eq!(ls["state"], "failed");
    assert_eq!(ls["failureReason"], "timeout");
}

/// A destination this adapter cannot push to is refused before any
/// process starts, as `"rejected"`; a missing ffmpeg is `"unavailable"`.
#[tokio::test]
async fn start_failures_map_to_failure_reasons() {
    let runner = FakeRunner::new();
    let media = FfmpegMedia::with_runner(config(), runner.clone());
    let b = backend();
    create(
        &b,
        json!({"callId": "C1", "streamUri": "rtmp://live.example.com/app", "streamKey": "k"}),
    )
    .await;
    let mut ls = started_livestream(&b);
    ls.stream_uri = Some("udp://10.0.0.1:1234".to_owned());
    let err = media
        .start_livestream(&ls, &Source::test_pattern())
        .await
        .unwrap_err();
    assert!(matches!(err, FfmpegError::UnsupportedScheme(_)), "{err:?}");
    assert_eq!(err.failure_reason(), LivestreamFailureReason::Rejected);
    ls.stream_uri = None;
    let err = media
        .start_livestream(&ls, &Source::test_pattern())
        .await
        .unwrap_err();
    assert!(matches!(err, FfmpegError::MissingStreamUri), "{err:?}");
    assert!(runner.spawned().is_empty());

    let missing = FfmpegMedia::with_runner(config(), FakeRunner::new().missing());
    let id = create(&b, json!({"callId": "C1", "mode": "hosted"})).await;
    let err = missing
        .start_livestream(&started_livestream(&b), &Source::test_pattern())
        .await
        .unwrap_err();
    assert!(matches!(err, FfmpegError::Spawn { .. }), "{err:?}");
    assert_eq!(err.failure_reason(), LivestreamFailureReason::Unavailable);
    let caller = system();
    MediaReporter::new(&b, &caller, Id::from(ACCOUNT))
        .livestream_failed(&id, err.failure_reason())
        .await
        .unwrap();
    assert_eq!(
        livestream_get(&b, &id).await["failureReason"],
        "unavailable"
    );
}

/// A process that ignores the stop request is killed after the grace.
#[tokio::test]
async fn hung_process_is_killed() {
    let runner = FakeRunner::new().hang();
    let media = FfmpegMedia::with_runner(config(), runner.clone());
    let b = backend();
    let id = create(&b, json!({"callId": "C1", "mode": "hosted"})).await;
    media
        .start_livestream(&started_livestream(&b), &Source::test_pattern())
        .await
        .unwrap();
    // Starting it again is a no-op.
    media
        .start_livestream(&started_livestream(&b), &Source::test_pattern())
        .await
        .unwrap();
    assert_eq!(runner.spawned().len(), 1);
    media.stop_livestream(&id).await.unwrap();
    let ctl = runner.last();
    let ctl = ctl.ctl();
    assert!(ctl.stop_requested && ctl.killed, "{ctl:?}");
}
