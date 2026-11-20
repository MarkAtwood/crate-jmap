//! The recording adapter end to end without ffmpeg: create, pause,
//! resume and stop through the `jmap-vtc-server` handlers, the effects
//! applied to the adapter over a fake runner, and the file reported
//! through `MediaReporter`.
//!
//! Oracles: draft-atwood-jmap-vtc-00 §Pausing, Resuming, and Stopping a
//! Recording ("a server MAY produce it by recording each unpaused
//! interval separately and joining them"; `"stopped"` → `"processing"` →
//! `"available"` with `blobId`, `size`, `duration` and `mediaType`); the
//! ffmpeg concat demuxer documentation (`file '<path>'` lines, `'\''` for
//! a quote).

mod common;

use std::time::Duration;

use common::{
    FakeRunner, backend, config, last_effect, recording_get, recording_set, started_recording,
    system,
};
use jmap_types::Id;
use jmap_vtc_ffmpeg::args::Source;
use jmap_vtc_ffmpeg::{FfmpegError, FfmpegMedia};
use jmap_vtc_server::MediaEffect;
use jmap_vtc_server::media_report::MediaReporter;
use jmap_vtc_server::memory::MemoryBackend;
use serde_json::{Value, json};

const ACCOUNT: &str = common::ACCOUNT;

async fn create(b: &MemoryBackend) -> Id {
    let r = recording_set(b, json!({"create": {"r1": {"callId": "C1"}}})).await;
    Id::from(
        r["created"]["r1"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{r}")),
    )
}

async fn set_state(b: &MemoryBackend, id: &Id, state: &str) -> Option<MediaEffect> {
    let r: Value = recording_set(b, json!({"update": {id.as_ref(): {"state": state}}})).await;
    assert!(r["notUpdated"].is_null(), "{r}");
    last_effect(b)
}

/// Record, pause, resume, stop: two parts joined into one MP4 that is
/// reported `"available"`.
#[tokio::test]
async fn paused_recording_is_joined() {
    let runner = FakeRunner::new();
    let media = FfmpegMedia::with_runner(config(), runner.clone());
    let b = backend();
    let id = create(&b).await;
    media
        .start_recording(&started_recording(&b), &Source::test_pattern())
        .await
        .unwrap();
    let first = runner.last();
    assert_eq!(
        first.dir,
        media
            .config()
            .work_root
            .join("recordings")
            .join(id.as_ref() as &str)
    );
    assert_eq!(
        first.args[first.args.len() - 3..],
        ["-f", "matroska", "part-0001.mkv"]
    );

    let Some(MediaEffect::PauseRecording(r)) = set_state(&b, &id, "paused").await else {
        panic!("no pause effect");
    };
    media.pause_recording(&r.id).await.unwrap();
    assert!(first.ctl().stop_requested);
    // Pausing again changes nothing.
    media.pause_recording(&id).await.unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;

    let Some(MediaEffect::ResumeRecording(r)) = set_state(&b, &id, "recording").await else {
        panic!("no resume effect");
    };
    media.resume_recording(&r.id).await.unwrap();
    media.resume_recording(&id).await.unwrap();
    let second = runner.last();
    assert_eq!(second.args.last().unwrap(), "part-0002.mkv");
    assert_eq!(second.dir, first.dir);
    assert_eq!(runner.spawned().len(), 2);

    let Some(MediaEffect::StopRecording(r)) = set_state(&b, &id, "stopped").await else {
        panic!("no stop effect");
    };
    let caller = system();
    let reporter = MediaReporter::new(&b, &caller, Id::from(ACCOUNT));
    reporter.recording_processing(&r.id, None).await.unwrap();
    let file = media.stop_recording(&r.id).await.unwrap();
    assert!(second.ctl().stop_requested);

    let concat = runner.last();
    assert_eq!(concat.after("-f"), "concat");
    assert_eq!(concat.after("-i"), "parts.txt");
    assert_eq!(
        std::fs::read_to_string(concat.dir.join("parts.txt")).unwrap(),
        "file 'part-0001.mkv'\nfile 'part-0002.mkv'\n"
    );
    assert_eq!(file.path, first.dir.join("recording.mp4"));
    assert_eq!(file.media_type, "video/mp4");
    assert_eq!(file.size, std::fs::metadata(&file.path).unwrap().len());

    reporter
        .recording_available(&id, file.recording_file(Id::from("B1")))
        .await
        .unwrap();
    let rec = recording_get(&b, &id).await;
    assert_eq!(rec["state"], "available");
    assert_eq!(rec["blobId"], "B1");
    assert_eq!(rec["mediaType"], "video/mp4");
    assert_eq!(rec["size"], json!(file.size));

    // The recording is gone from the adapter.
    assert!(matches!(
        media.stop_recording(&id).await,
        Err(FfmpegError::NotRunning(_))
    ));
    assert!(matches!(
        media.pause_recording(&id).await,
        Err(FfmpegError::NotRunning(_))
    ));
}

/// A part's process that ends on its own stops the recording without a
/// moderator; the stop effect then joins what was written.
#[tokio::test]
async fn crashed_part_stops_the_recording() {
    let runner = FakeRunner::new();
    let media = FfmpegMedia::with_runner(config(), runner.clone());
    let b = backend();
    let id = create(&b).await;
    media
        .start_recording(&started_recording(&b), &Source::test_pattern())
        .await
        .unwrap();
    runner.last().exit(Some(1));

    let caller = system();
    let reporter = MediaReporter::new(&b, &caller, Id::from(ACCOUNT));
    media.watch_recording(&reporter, &id).await.unwrap();
    assert_eq!(recording_get(&b, &id).await["state"], "stopped");
    let Some(MediaEffect::StopRecording(r)) = last_effect(&b) else {
        panic!("no stop effect");
    };
    let file = media.stop_recording(&r.id).await.unwrap();
    assert!(file.path.exists());
    // Nothing left to watch.
    media.watch_recording(&reporter, &id).await.unwrap();
}

/// A stop with no media written, and a failed join, are errors the
/// deployment reports with `recording_failed`.
#[tokio::test]
async fn empty_and_failed_recordings() {
    let b = backend();
    let caller = system();
    let reporter = MediaReporter::new(&b, &caller, Id::from(ACCOUNT));

    let silent = FfmpegMedia::with_runner(config(), FakeRunner::new().silent());
    let id = create(&b).await;
    silent
        .start_recording(&started_recording(&b), &Source::test_pattern())
        .await
        .unwrap();
    let err = silent.stop_recording(&id).await.unwrap_err();
    assert!(matches!(err, FfmpegError::EmptyRecording(_)), "{err:?}");

    let broken = FfmpegMedia::with_runner(config(), FakeRunner::new().concat_exit(1));
    let id = create(&b).await;
    broken
        .start_recording(&started_recording(&b), &Source::test_pattern())
        .await
        .unwrap();
    set_state(&b, &id, "stopped").await;
    reporter.recording_processing(&id, None).await.unwrap();
    let err = broken.stop_recording(&id).await.unwrap_err();
    assert!(
        matches!(err, FfmpegError::Exited(e) if e.code() == Some(1)),
        "{err:?}"
    );
    reporter.recording_failed(&id).await.unwrap();
    assert_eq!(recording_get(&b, &id).await["state"], "failed");
}
