//! The `VTCLivestream` and `VTCRecording` adapter over ffmpeg processes.

use std::collections::HashMap;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use jmap_types::Id;
use jmap_vtc_server::VtcBackend;
use jmap_vtc_server::media_report::{
    LivestreamReport, MediaReporter, RecordingFile, RecordingReport,
};
use jmap_vtc_types::{
    LatencyHint, LivestreamFailureReason, LivestreamMode, PlaybackProtocol, VtcLivestream,
    VtcPlaybackEndpoint, VtcRecording,
};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use url::Url;

use crate::FfmpegError;
use crate::args::{
    self, CONCAT_LIST, Encoding, HlsSettings, PLAYLIST, PROGRESS, PushTarget, RECORDING_FILE,
    RECORDING_MEDIA_TYPE, Source,
};
use crate::process::{MediaProcess, ProcessRunner, TokioRunner, finish};

/// How long a livestream may stay `"starting"` before
/// [`FfmpegMedia::watch_livestream`] fails it with `"timeout"`.
pub const DEFAULT_START_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a graceful stop may take before the process is killed.
pub const DEFAULT_STOP_GRACE: Duration = Duration::from_secs(10);

/// How often the watchers look at the processes and their output files.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Where [`FfmpegMedia`] writes and how it runs ffmpeg.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct FfmpegConfig {
    /// The ffmpeg program, looked up on `PATH` unless it has a directory.
    pub program: PathBuf,
    /// The static HLS origin's document root: hosted livestream `{id}`
    /// writes `{hls_root}/{id}/index.m3u8` and its segments there.
    pub hls_root: PathBuf,
    /// The URL `hls_root` is served at, ending in `/`: the playback URL
    /// of livestream `{id}` is `{base_url}{id}/index.m3u8`.
    pub base_url: Url,
    /// A private directory for recordings (`{work_root}/recordings/{id}/`)
    /// and push-mode progress files (`{work_root}/livestreams/{id}/`).
    /// Must not be served.
    pub work_root: PathBuf,
    /// The encoding of every output.
    pub encoding: Encoding,
    /// The HLS muxer settings of hosted livestreams.
    pub hls: HlsSettings,
    /// See [`DEFAULT_START_TIMEOUT`].
    pub start_timeout: Duration,
    /// See [`DEFAULT_STOP_GRACE`].
    pub stop_grace: Duration,
    /// See [`DEFAULT_POLL_INTERVAL`].
    pub poll_interval: Duration,
}

impl FfmpegConfig {
    /// HLS under `hls_root`, served at `base_url` (a `/` is added if its
    /// path lacks one), private files under `work_root`, `ffmpeg` from
    /// `PATH`, and the defaults for the rest.
    pub fn new(
        hls_root: impl Into<PathBuf>,
        mut base_url: Url,
        work_root: impl Into<PathBuf>,
    ) -> Self {
        if !base_url.path().ends_with('/') {
            let path = format!("{}/", base_url.path());
            base_url.set_path(&path);
        }
        Self {
            program: PathBuf::from("ffmpeg"),
            hls_root: hls_root.into(),
            base_url,
            work_root: work_root.into(),
            encoding: Encoding::default(),
            hls: HlsSettings::default(),
            start_timeout: DEFAULT_START_TIMEOUT,
            stop_grace: DEFAULT_STOP_GRACE,
            poll_interval: DEFAULT_POLL_INTERVAL,
        }
    }

    /// This configuration running `program`.
    pub fn with_program(self, program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            ..self
        }
    }

    /// This configuration encoding with `encoding`.
    pub fn with_encoding(self, encoding: Encoding) -> Self {
        Self { encoding, ..self }
    }

    /// This configuration with HLS settings `hls`.
    pub fn with_hls(self, hls: HlsSettings) -> Self {
        Self { hls, ..self }
    }

    /// This configuration with start timeout `timeout`.
    pub fn with_start_timeout(self, timeout: Duration) -> Self {
        Self {
            start_timeout: timeout,
            ..self
        }
    }

    /// This configuration with stop grace `grace`.
    pub fn with_stop_grace(self, grace: Duration) -> Self {
        Self {
            stop_grace: grace,
            ..self
        }
    }

    /// This configuration polling every `interval`.
    pub fn with_poll_interval(self, interval: Duration) -> Self {
        Self {
            poll_interval: interval,
            ..self
        }
    }
}

/// A finished recording file, from [`FfmpegMedia::stop_recording`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedFile {
    /// The file, `{work_root}/recordings/{id}/recording.mp4`. Its
    /// directory also holds the parts; remove it once the file is stored.
    pub path: PathBuf,
    /// The file size in bytes.
    pub size: u64,
    /// The recorded time in whole seconds: the wall-clock time the parts
    /// ran, paused intervals left out.
    pub duration: u64,
    /// The MIME type, `"video/mp4"`.
    pub media_type: String,
}

impl RecordedFile {
    /// The [`RecordingFile`] to report with
    /// [`MediaReporter::recording_available`] once this file is stored as
    /// blob `blob_id`.
    pub fn recording_file(&self, blob_id: Id) -> RecordingFile {
        RecordingFile::new(blob_id, self.size, self.duration, self.media_type.clone())
    }
}

/// What tells a starting livestream is live.
#[derive(Debug, Clone, Copy)]
enum Ready {
    /// The HLS playlist lists a segment.
    Playlist,
    /// The progress file shows bytes written to the output.
    Progress,
}

struct Livestream<P> {
    process: P,
    ready: Ready,
    ready_file: PathBuf,
    endpoints: Vec<VtcPlaybackEndpoint>,
    started: Instant,
    live: bool,
    /// A directory to remove when the livestream ends (push mode).
    scratch: Option<PathBuf>,
}

struct Part<P> {
    process: P,
    started: Instant,
}

struct Recording<P> {
    dir: PathBuf,
    source: Source,
    parts: Vec<String>,
    current: Option<Part<P>>,
    recorded: Duration,
}

/// Hosted and push `VTCLivestream`s and `VTCRecording`s on ffmpeg
/// processes, one process per livestream and per recording part.
///
/// Unlike a cloud adapter, this one owns live processes, so it keeps a
/// table of them keyed by livestream and recording id; share one
/// `FfmpegMedia` (in an `Arc`) between the backend's
/// [`run_media_effect`](VtcBackend::run_media_effect) and the watcher
/// tasks.
pub struct FfmpegMedia<R: ProcessRunner = TokioRunner> {
    runner: R,
    config: FfmpegConfig,
    livestreams: Mutex<HashMap<Id, Livestream<R::Process>>>,
    recordings: Mutex<HashMap<Id, Recording<R::Process>>>,
}

impl<R: ProcessRunner> std::fmt::Debug for FfmpegMedia<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FfmpegMedia")
            .field("config", &self.config)
            .field("livestreams", &lock(&self.livestreams).len())
            .field("recordings", &lock(&self.recordings).len())
            .finish_non_exhaustive()
    }
}

impl FfmpegMedia<TokioRunner> {
    /// An adapter running the real ffmpeg with `config`.
    pub fn new(config: FfmpegConfig) -> Self {
        Self::with_runner(config, TokioRunner::new())
    }
}

impl<R: ProcessRunner> FfmpegMedia<R> {
    /// An adapter starting its processes with `runner`.
    pub fn with_runner(config: FfmpegConfig, runner: R) -> Self {
        Self {
            runner,
            config,
            livestreams: Mutex::new(HashMap::new()),
            recordings: Mutex::new(HashMap::new()),
        }
    }

    /// The configuration.
    pub fn config(&self) -> &FfmpegConfig {
        &self.config
    }

    /// The HLS endpoint of hosted livestream `id`: `{base_url}{id}/index.m3u8`,
    /// `latencyHint` `"standard"` (HLS with whole-segment buffering is
    /// several segments behind), no credential: every viewer gets the
    /// same URL, so [`VtcBackend::playback_endpoints_for`] keeps its
    /// default.
    ///
    /// # Errors
    ///
    /// [`FfmpegError::UnsafeId`] for an id that cannot name a directory.
    pub fn playback_endpoint(&self, id: &Id) -> Result<VtcPlaybackEndpoint, FfmpegError> {
        let name = safe_name(id)?;
        let url = self
            .config
            .base_url
            .join(&format!("{name}/{PLAYLIST}"))
            .map_err(|_| FfmpegError::UnsafeId(id.clone()))?;
        Ok(VtcPlaybackEndpoint::new(
            PlaybackProtocol::Hls,
            url.to_string(),
            LatencyHint::Standard,
        ))
    }

    /// Start `livestream` from `source`
    /// ([`MediaEffect::StartLivestream`](jmap_vtc_server::MediaEffect::StartLivestream)):
    ///
    /// - hosted: HLS into `{hls_root}/{id}/` ([`args::hls_args`]);
    /// - push: to its `streamUri` with its `streamKey`
    ///   ([`PushTarget::new`], [`args::push_args`]).
    ///
    /// Returns once the process runs; [`Self::watch_livestream`] reports
    /// when it is live. A livestream already running here is left alone.
    ///
    /// # Errors
    ///
    /// The livestream cannot be served ([`FfmpegError::UnsafeId`],
    /// [`FfmpegError::UnsupportedMode`], the [`PushTarget::new`] errors)
    /// or ffmpeg cannot be started. Report either with
    /// [`MediaReporter::livestream_failed`] and
    /// [`FfmpegError::failure_reason`].
    pub async fn start_livestream(
        &self,
        livestream: &VtcLivestream,
        source: &Source,
    ) -> Result<(), FfmpegError> {
        let id = &livestream.id;
        let name = safe_name(id)?;
        if lock(&self.livestreams).contains_key(id) {
            return Ok(());
        }
        let (dir, args, ready, ready_file, endpoints, scratch) = match &livestream.mode {
            LivestreamMode::Hosted => {
                let dir = self.config.hls_root.join(name);
                let args = args::hls_args(source, &self.config.encoding, &self.config.hls);
                let playlist = dir.join(PLAYLIST);
                let endpoints = vec![self.playback_endpoint(id)?];
                (dir, args, Ready::Playlist, playlist, endpoints, None)
            }
            LivestreamMode::Push => {
                let uri = livestream
                    .stream_uri
                    .as_deref()
                    .ok_or(FfmpegError::MissingStreamUri)?;
                let target = PushTarget::new(uri, livestream.stream_key.as_deref())?;
                let dir = self.config.work_root.join("livestreams").join(name);
                let args = args::push_args(source, &self.config.encoding, &target);
                let progress = dir.join(PROGRESS);
                (
                    dir.clone(),
                    args,
                    Ready::Progress,
                    progress,
                    Vec::new(),
                    Some(dir),
                )
            }
            other => return Err(FfmpegError::UnsupportedMode(other.clone())),
        };
        create_dir(&dir).await?;
        remove_file(&ready_file).await?;
        let process = self.spawn(&args, &dir)?;
        lock(&self.livestreams)
            .entry(id.clone())
            .or_insert(Livestream {
                process,
                ready,
                ready_file,
                endpoints,
                started: Instant::now(),
                live: false,
                scratch,
            });
        Ok(())
    }

    /// Stop livestream `id`
    /// ([`MediaEffect::StopLivestream`](jmap_vtc_server::MediaEffect::StopLivestream)):
    /// ask ffmpeg to finish (an HLS playlist then ends with
    /// `EXT-X-ENDLIST`), and kill it after the stop grace. Idempotent: a
    /// livestream not running here is fine. The HLS files stay; remove
    /// `{hls_root}/{id}/` when viewers no longer need them.
    ///
    /// # Errors
    ///
    /// [`FfmpegError::Process`] when the process cannot be stopped.
    pub async fn stop_livestream(&self, id: &Id) -> Result<(), FfmpegError> {
        let Some(mut ls) = lock(&self.livestreams).remove(id) else {
            return Ok(());
        };
        let result = finish(&mut ls.process, self.config.stop_grace).await;
        remove_scratch(ls.scratch).await;
        result.map(|_| ()).map_err(FfmpegError::Process)
    }

    /// Supervise livestream `id` until it ends, reporting through
    /// `reporter`:
    ///
    /// | Observed | Report |
    /// |---|---|
    /// | hosted: the playlist lists a segment | `livestream_live` with [`Self::playback_endpoint`] |
    /// | push: the progress file shows output bytes | `livestream_live` with no endpoints ("Always empty in push mode") |
    /// | still not live after the start timeout | the process is killed; `livestream_failed`, `"timeout"` |
    /// | the process ends before it is live | `livestream_failed`, `"rejected"` (bad input, or the destination refused) |
    /// | the process ends while live | `livestream_failed`, `"interrupted"` ("The stream was live and was lost") |
    /// | [`Self::stop_livestream`] | nothing |
    ///
    /// Spawn it after [`Self::start_livestream`]; it returns when the
    /// livestream is no longer running here. A failure report requests
    /// [`MediaEffect::StopLivestream`](jmap_vtc_server::MediaEffect::StopLivestream),
    /// whose [`Self::stop_livestream`] then finds nothing to stop.
    ///
    /// # Errors
    ///
    /// The report's error. [`ReportError::WrongState`](jmap_vtc_server::media_report::ReportError::WrongState)
    /// means the livestream changed state meanwhile (a moderator's stop
    /// racing the report) and is expected.
    pub async fn watch_livestream<B: VtcBackend>(
        &self,
        reporter: &MediaReporter<'_, B>,
        id: &Id,
    ) -> LivestreamReport<B::Error> {
        loop {
            match self.observe_livestream(id) {
                Observed::Gone => return Ok(()),
                Observed::Ended { live, scratch } => {
                    remove_scratch(scratch).await;
                    let reason = if live {
                        LivestreamFailureReason::Interrupted
                    } else {
                        LivestreamFailureReason::Rejected
                    };
                    return reporter.livestream_failed(id, reason).await;
                }
                Observed::Live => {}
                Observed::Starting {
                    ready,
                    ready_file,
                    started,
                } => {
                    if is_ready(ready, &ready_file).await {
                        let Some(endpoints) = self.mark_live(id) else {
                            return Ok(());
                        };
                        reporter.livestream_live(id, endpoints).await?;
                    } else if started.elapsed() >= self.config.start_timeout {
                        let Some(mut ls) = lock(&self.livestreams).remove(id) else {
                            return Ok(());
                        };
                        // Nothing worth a clean ending was written.
                        if ls.process.kill().await.is_ok() {
                            let _ = ls.process.wait().await;
                        }
                        remove_scratch(ls.scratch).await;
                        return reporter
                            .livestream_failed(id, LivestreamFailureReason::Timeout)
                            .await;
                    }
                }
            }
            tokio::time::sleep(self.config.poll_interval).await;
        }
    }

    /// Start recording `recording` from `source`
    /// ([`MediaEffect::StartRecording`](jmap_vtc_server::MediaEffect::StartRecording)):
    /// its first part into `{work_root}/recordings/{id}/part-0001.mkv`
    /// ([`args::record_args`]). A recording already running here is left
    /// alone.
    ///
    /// # Errors
    ///
    /// [`FfmpegError::UnsafeId`], or ffmpeg cannot be started. Report
    /// either with [`MediaReporter::recording_stopped`] and
    /// [`MediaReporter::recording_failed`].
    pub async fn start_recording(
        &self,
        recording: &VtcRecording,
        source: &Source,
    ) -> Result<(), FfmpegError> {
        let id = &recording.id;
        let name = safe_name(id)?;
        if lock(&self.recordings).contains_key(id) {
            return Ok(());
        }
        let dir = self.config.work_root.join("recordings").join(name);
        create_dir(&dir).await?;
        let part = args::part_name(1);
        let process = self.spawn(
            &args::record_args(source, &self.config.encoding, &part),
            &dir,
        )?;
        lock(&self.recordings)
            .entry(id.clone())
            .or_insert(Recording {
                dir,
                source: source.clone(),
                parts: vec![part],
                current: Some(Part {
                    process,
                    started: Instant::now(),
                }),
                recorded: Duration::ZERO,
            });
        Ok(())
    }

    /// Pause recording `id`
    /// ([`MediaEffect::PauseRecording`](jmap_vtc_server::MediaEffect::PauseRecording)):
    /// finish the current part. Nothing is recorded until
    /// [`Self::resume_recording`]; the paused interval is left out of the
    /// file, which draft-atwood-jmap-vtc-00 allows ("a server MAY produce
    /// it by recording each unpaused interval separately and joining
    /// them"). Pausing a paused recording is fine.
    ///
    /// # Errors
    ///
    /// [`FfmpegError::NotRunning`], or [`FfmpegError::Process`] when the
    /// part cannot be stopped.
    pub async fn pause_recording(&self, id: &Id) -> Result<(), FfmpegError> {
        let part = {
            let mut map = lock(&self.recordings);
            let rec = map
                .get_mut(id)
                .ok_or_else(|| FfmpegError::NotRunning(id.clone()))?;
            rec.current.take()
        };
        let Some(mut part) = part else {
            return Ok(());
        };
        let result = finish(&mut part.process, self.config.stop_grace).await;
        if let Some(rec) = lock(&self.recordings).get_mut(id) {
            rec.recorded += part.started.elapsed();
        }
        result.map(|_| ()).map_err(FfmpegError::Process)
    }

    /// Resume recording `id`
    /// ([`MediaEffect::ResumeRecording`](jmap_vtc_server::MediaEffect::ResumeRecording)):
    /// start its next part from the recording's source. Resuming a
    /// recording that is not paused is fine.
    ///
    /// # Errors
    ///
    /// [`FfmpegError::NotRunning`], or ffmpeg cannot be started.
    pub async fn resume_recording(&self, id: &Id) -> Result<(), FfmpegError> {
        let (dir, source, part) = {
            let map = lock(&self.recordings);
            let rec = map
                .get(id)
                .ok_or_else(|| FfmpegError::NotRunning(id.clone()))?;
            if rec.current.is_some() {
                return Ok(());
            }
            let n = u32::try_from(rec.parts.len() + 1).unwrap_or(u32::MAX);
            (rec.dir.clone(), rec.source.clone(), args::part_name(n))
        };
        let process = self.spawn(
            &args::record_args(&source, &self.config.encoding, &part),
            &dir,
        )?;
        let mut map = lock(&self.recordings);
        if let Some(rec) = map.get_mut(id)
            && rec.current.is_none()
        {
            rec.parts.push(part);
            rec.current = Some(Part {
                process,
                started: Instant::now(),
            });
        }
        Ok(())
    }

    /// Stop recording `id`
    /// ([`MediaEffect::StopRecording`](jmap_vtc_server::MediaEffect::StopRecording))
    /// and make its file: finish the current part, then join the parts
    /// into `recording.mp4` without re-encoding ([`args::concat_args`]).
    ///
    /// Report [`MediaReporter::recording_processing`] before calling it
    /// (the join can take a while), store the returned file as a blob,
    /// then report [`MediaReporter::recording_available`] with
    /// [`RecordedFile::recording_file`], or
    /// [`MediaReporter::recording_failed`] on an error.
    ///
    /// # Errors
    ///
    /// [`FfmpegError::NotRunning`] (also for a second stop),
    /// [`FfmpegError::EmptyRecording`] when no part wrote media,
    /// [`FfmpegError::Exited`] when the join failed, and I/O errors.
    pub async fn stop_recording(&self, id: &Id) -> Result<RecordedFile, FfmpegError> {
        let mut rec = lock(&self.recordings)
            .remove(id)
            .ok_or_else(|| FfmpegError::NotRunning(id.clone()))?;
        if let Some(mut part) = rec.current.take() {
            finish(&mut part.process, self.config.stop_grace)
                .await
                .map_err(FfmpegError::Process)?;
            rec.recorded += part.started.elapsed();
        }
        let mut written = Vec::new();
        for part in &rec.parts {
            if tokio::fs::metadata(rec.dir.join(part))
                .await
                .is_ok_and(|m| m.len() > 0)
            {
                written.push(part.as_str());
            }
        }
        if written.is_empty() {
            return Err(FfmpegError::EmptyRecording(id.clone()));
        }
        let list = rec.dir.join(CONCAT_LIST);
        tokio::fs::write(&list, args::concat_list(&written))
            .await
            .map_err(|source| FfmpegError::Io { path: list, source })?;
        let output = rec.dir.join(RECORDING_FILE);
        remove_file(&output).await?;
        let exit = self
            .spawn(&args::concat_args(), &rec.dir)?
            .wait()
            .await
            .map_err(FfmpegError::Process)?;
        if !exit.success() {
            return Err(FfmpegError::Exited(exit));
        }
        let size = tokio::fs::metadata(&output)
            .await
            .map_err(|source| FfmpegError::Io {
                path: output.clone(),
                source,
            })?
            .len();
        Ok(RecordedFile {
            path: output,
            size,
            duration: (rec.recorded + Duration::from_millis(500)).as_secs(),
            media_type: RECORDING_MEDIA_TYPE.to_owned(),
        })
    }

    /// Supervise recording `id` until it stops. When the running part's
    /// process ends without a pause or stop, it reports
    /// [`MediaReporter::recording_stopped`] (the recording "stopped
    /// without a moderator") and returns; that report requests
    /// [`MediaEffect::StopRecording`](jmap_vtc_server::MediaEffect::StopRecording),
    /// whose [`Self::stop_recording`] joins the parts written so far.
    /// After [`Self::stop_recording`] it returns `Ok`.
    ///
    /// # Errors
    ///
    /// The report's error.
    pub async fn watch_recording<B: VtcBackend>(
        &self,
        reporter: &MediaReporter<'_, B>,
        id: &Id,
    ) -> RecordingReport<B::Error> {
        loop {
            let ended = {
                let mut map = lock(&self.recordings);
                let Some(rec) = map.get_mut(id) else {
                    return Ok(());
                };
                let ended = rec
                    .current
                    .as_mut()
                    .is_some_and(|p| !matches!(p.process.try_wait(), Ok(None)));
                if ended && let Some(part) = rec.current.take() {
                    rec.recorded += part.started.elapsed();
                }
                ended
            };
            if ended {
                return reporter.recording_stopped(id).await;
            }
            tokio::time::sleep(self.config.poll_interval).await;
        }
    }

    fn spawn(&self, args: &[std::ffi::OsString], dir: &Path) -> Result<R::Process, FfmpegError> {
        self.runner
            .spawn(&self.config.program, args, dir)
            .map_err(|source| FfmpegError::Spawn {
                program: self.config.program.clone(),
                source,
            })
    }

    fn observe_livestream(&self, id: &Id) -> Observed {
        let mut map = lock(&self.livestreams);
        let Some(ls) = map.get_mut(id) else {
            return Observed::Gone;
        };
        if !matches!(ls.process.try_wait(), Ok(None)) {
            let ls = map.remove(id).expect("present");
            return Observed::Ended {
                live: ls.live,
                scratch: ls.scratch,
            };
        }
        if ls.live {
            return Observed::Live;
        }
        Observed::Starting {
            ready: ls.ready,
            ready_file: ls.ready_file.clone(),
            started: ls.started,
        }
    }

    /// Mark livestream `id` live and return its endpoints, or `None` if
    /// it is no longer running here.
    fn mark_live(&self, id: &Id) -> Option<Vec<VtcPlaybackEndpoint>> {
        let mut map = lock(&self.livestreams);
        let ls = map.get_mut(id)?;
        ls.live = true;
        Some(ls.endpoints.clone())
    }
}

enum Observed {
    Gone,
    Ended {
        live: bool,
        scratch: Option<PathBuf>,
    },
    Live,
    Starting {
        ready: Ready,
        ready_file: PathBuf,
        started: Instant,
    },
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // The tables stay consistent across a panic in another holder: every
    // critical section is a single insert, remove or field update.
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `id` as a directory name: the RFC 8620 §1.2 `Id` syntax, "1 and 255
/// octets", "only characters from the URL and Filename Safe base64
/// alphabet". It has no `.` or `/`, so it cannot leave its root.
fn safe_name(id: &Id) -> Result<&str, FfmpegError> {
    let s: &str = id.as_ref();
    if (1..=255).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        Ok(s)
    } else {
        Err(FfmpegError::UnsafeId(id.clone()))
    }
}

async fn create_dir(dir: &Path) -> Result<(), FfmpegError> {
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|source| FfmpegError::Io {
            path: dir.to_owned(),
            source,
        })
}

/// Remove `path` if it exists.
async fn remove_file(path: &Path) -> Result<(), FfmpegError> {
    match tokio::fs::remove_file(path).await {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(FfmpegError::Io {
            path: path.to_owned(),
            source: e,
        }),
        _ => Ok(()),
    }
}

/// Best effort: a leftover progress file is harmless.
async fn remove_scratch(dir: Option<PathBuf>) {
    if let Some(dir) = dir {
        let _ = tokio::fs::remove_dir_all(dir).await;
    }
}

/// The last 4 KiB of `path`, or `None` if it cannot be read.
async fn tail(path: &Path) -> Option<String> {
    const TAIL: u64 = 4096;
    let mut file = tokio::fs::File::open(path).await.ok()?;
    let len = file.metadata().await.ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(TAIL)))
        .await
        .ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).await.ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

async fn is_ready(ready: Ready, path: &Path) -> bool {
    let Some(text) = tail(path).await else {
        return false;
    };
    match ready {
        Ready::Playlist => playlist_has_segment(&text),
        Ready::Progress => progress_shows_output(&text),
    }
}

/// An HLS media playlist with at least one `#EXTINF` (RFC 8216 §4.3.2.1).
fn playlist_has_segment(text: &str) -> bool {
    text.lines().any(|l| l.starts_with("#EXTINF:"))
}

/// The last `total_size=` of an ffmpeg `-progress` file is positive: the
/// output has taken bytes.
fn progress_shows_output(text: &str) -> bool {
    text.lines()
        .rev()
        .find_map(|l| l.strip_prefix("total_size="))
        .and_then(|v| v.trim().parse::<i64>().ok())
        .is_some_and(|n| n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_names() {
        assert_eq!(
            safe_name(&Id::from("01J4XQ7VN0SYWV2TTFHJVW7JK0")).unwrap(),
            "01J4XQ7VN0SYWV2TTFHJVW7JK0"
        );
        assert_eq!(safe_name(&Id::from("a-b_c")).unwrap(), "a-b_c");
        for bad in ["", "..", "../etc", "a/b", "a.b", "a b", "é"] {
            assert!(safe_name(&Id::from(bad)).is_err(), "{bad:?}");
        }
        assert!(safe_name(&Id::from("x".repeat(256).as_str())).is_err());
    }

    #[test]
    fn playlist_readiness() {
        assert!(!playlist_has_segment("#EXTM3U\n#EXT-X-VERSION:3\n"));
        assert!(playlist_has_segment(
            "#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXTINF:2.000000,\nseg00000.ts\n"
        ));
    }

    /// ffmpeg `-progress` blocks: the first one may show no output yet.
    #[test]
    fn progress_readiness() {
        let first = "frame=0\nfps=0.00\ntotal_size=0\nout_time_us=0\nprogress=continue\n";
        assert!(!progress_shows_output(first));
        let later = format!("{first}frame=60\nfps=30.00\ntotal_size=48\nprogress=continue\n");
        assert!(progress_shows_output(&later));
        assert!(!progress_shows_output("total_size=N/A\n"));
        assert!(!progress_shows_output(""));
    }

    #[test]
    fn base_url_gets_a_trailing_slash() {
        let c = FfmpegConfig::new(
            "/srv/hls",
            Url::parse("https://cdn.example.com/live").unwrap(),
            "/var/lib/vtc",
        );
        assert_eq!(c.base_url.as_str(), "https://cdn.example.com/live/");
        let media = FfmpegMedia::new(c);
        let ep = media.playback_endpoint(&Id::from("L1")).unwrap();
        assert_eq!(ep.url, "https://cdn.example.com/live/L1/index.m3u8");
        assert_eq!(ep.protocol, PlaybackProtocol::Hls);
        assert!(ep.credential.is_none());
        assert!(media.playback_endpoint(&Id::from("../L1")).is_err());
    }
}
