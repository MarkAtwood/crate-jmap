//! The adapter against the real ffmpeg binary, from its synthetic
//! `testsrc2`/`sine` source: no camera, no SFU, no network beyond
//! loopback.
//!
//! Compiled only with the `real-ffmpeg` feature and `#[ignore]`d besides
//! (decision 57), so plain `cargo test` never needs ffmpeg. Run with
//!
//! ```text
//! cargo test -p jmap-vtc-ffmpeg --features real-ffmpeg -- --ignored
//! ```
//!
//! on a machine with ffmpeg (with libx264) on `PATH`.
//!
//! Oracles: RFC 8216 (an HLS media playlist lists segments with
//! `#EXTINF` and ends with `#EXT-X-ENDLIST`), ISO/IEC 14496-12 (an MP4
//! file starts with an `ftyp` box), draft-atwood-jmap-vtc-00 §Livestream
//! States.

#![cfg(feature = "real-ffmpeg")]

mod common;

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use common::{
    backend, config, last_effect, livestream_get, livestream_set, recording_set,
    started_livestream, started_recording, system,
};
use jmap_types::Id;
use jmap_vtc_ffmpeg::args::{Encoding, HlsSettings, Source, TestPattern};
use jmap_vtc_ffmpeg::process::{MediaProcess, ProcessRunner, TokioRunner};
use jmap_vtc_ffmpeg::{FfmpegConfig, FfmpegMedia};
use jmap_vtc_server::MediaEffect;
use jmap_vtc_server::media_report::MediaReporter;
use serde_json::json;

const ACCOUNT: &str = common::ACCOUNT;

fn real_config() -> FfmpegConfig {
    config()
        .with_encoding(Encoding::default().with_video_bitrate_kbps(500))
        .with_hls(HlsSettings::default().with_segment_secs(1))
        .with_start_timeout(Duration::from_secs(20))
        .with_stop_grace(Duration::from_secs(10))
        .with_poll_interval(Duration::from_millis(100))
}

fn small_pattern() -> Source {
    Source::TestPattern(TestPattern::default().with_size(320, 240))
}

/// A hosted livestream goes live once ffmpeg has written a segment, and
/// a stop ends the playlist with `EXT-X-ENDLIST`.
#[tokio::test]
#[ignore = "runs ffmpeg"]
async fn hosted_hls_from_the_test_source() {
    let media = FfmpegMedia::new(real_config());
    let b = backend();
    let r = livestream_set(
        &b,
        json!({"create": {"s1": {"callId": "C1", "mode": "hosted"}}}),
    )
    .await;
    let id = Id::from(r["created"]["s1"]["id"].as_str().unwrap());
    media
        .start_livestream(&started_livestream(&b), &small_pattern())
        .await
        .unwrap();

    let caller = system();
    let reporter = MediaReporter::new(&b, &caller, Id::from(ACCOUNT));
    let dir = media.config().hls_root.join(id.as_ref() as &str);
    let driver = async {
        loop {
            if livestream_get(&b, &id).await["state"] == "live" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let playlist = std::fs::read_to_string(dir.join("index.m3u8")).unwrap();
        assert!(playlist.contains("#EXTINF:"), "{playlist}");
        assert!(playlist.contains("#EXT-X-PROGRAM-DATE-TIME:"), "{playlist}");
        assert!(std::fs::metadata(dir.join("seg00000.ts")).unwrap().len() > 0);
        livestream_set(&b, json!({"update": {id.as_ref(): {"state": "stopped"}}})).await;
        let Some(MediaEffect::StopLivestream(ls)) = last_effect(&b) else {
            panic!("no stop effect");
        };
        media.stop_livestream(&ls.id).await.unwrap();
    };
    let (watched, ()) = tokio::join!(media.watch_livestream(&reporter, &id), driver);
    watched.unwrap();
    let playlist = std::fs::read_to_string(dir.join("index.m3u8")).unwrap();
    assert!(playlist.contains("#EXT-X-ENDLIST"), "{playlist}");
    assert_eq!(livestream_get(&b, &id).await["state"], "stopped");
}

/// A push livestream to a local RTMP listener (a second ffmpeg) goes live
/// from its progress file; one to a closed port is `"rejected"`.
#[tokio::test]
#[ignore = "runs ffmpeg"]
async fn push_to_a_local_rtmp_listener() {
    let work = common::scratch();
    let listen: Vec<OsString> = [
        "-hide_banner",
        "-loglevel",
        "warning",
        "-listen",
        "1",
        "-i",
        "rtmp://127.0.0.1:19351/app/sk_test",
        "-f",
        "null",
        "-",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    let mut listener = TokioRunner::new()
        .spawn(Path::new("ffmpeg"), &listen, &work)
        .unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;

    let media = FfmpegMedia::new(real_config());
    let b = backend();
    let r = livestream_set(
        &b,
        json!({"create": {"s1": {
            "callId": "C1",
            "streamUri": "rtmp://127.0.0.1:19351/app",
            "streamKey": "sk_test"
        }}}),
    )
    .await;
    let id = Id::from(r["created"]["s1"]["id"].as_str().unwrap());
    media
        .start_livestream(&started_livestream(&b), &small_pattern())
        .await
        .unwrap();
    let caller = system();
    let reporter = MediaReporter::new(&b, &caller, Id::from(ACCOUNT));
    let driver = async {
        loop {
            if livestream_get(&b, &id).await["state"] == "live" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        media.stop_livestream(&id).await.unwrap();
    };
    let (watched, ()) = tokio::join!(media.watch_livestream(&reporter, &id), driver);
    watched.unwrap();
    let _ = listener.kill().await;

    let r = livestream_set(
        &b,
        json!({"create": {"s2": {
            "callId": "C1",
            "streamUri": "rtmp://127.0.0.1:1/app",
            "streamKey": "k"
        }}}),
    )
    .await;
    let closed = Id::from(r["created"]["s2"]["id"].as_str().unwrap());
    media
        .start_livestream(&started_livestream(&b), &small_pattern())
        .await
        .unwrap();
    media.watch_livestream(&reporter, &closed).await.unwrap();
    let ls = livestream_get(&b, &closed).await;
    assert_eq!(ls["state"], "failed");
    assert_eq!(ls["failureReason"], "rejected");
}

/// Record, pause, resume and stop: the two parts join into one MP4.
#[tokio::test]
#[ignore = "runs ffmpeg"]
async fn paused_recording_joins_into_mp4() {
    let media = FfmpegMedia::new(real_config());
    let b = backend();
    let r = recording_set(&b, json!({"create": {"r1": {"callId": "C1"}}})).await;
    let id = Id::from(r["created"]["r1"]["id"].as_str().unwrap());
    media
        .start_recording(&started_recording(&b), &small_pattern())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(2)).await;
    media.pause_recording(&id).await.unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    media.resume_recording(&id).await.unwrap();
    tokio::time::sleep(Duration::from_secs(2)).await;
    let file = media.stop_recording(&id).await.unwrap();

    let bytes = std::fs::read(&file.path).unwrap();
    assert_eq!(&bytes[4..8], b"ftyp");
    assert_eq!(file.size, bytes.len() as u64);
    assert!((3..=6).contains(&file.duration), "{}", file.duration);
}
