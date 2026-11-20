//! Shared fixtures: a fake [`ProcessRunner`] that records every spawn and
//! writes the files ffmpeg would write, a `MemoryBackend` account with an
//! active call, and scratch directories. No ffmpeg is ever run.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use jmap_types::Id;
use jmap_vtc_ffmpeg::FfmpegConfig;
use jmap_vtc_ffmpeg::process::{MediaProcess, ProcessExit, ProcessRunner};
use jmap_vtc_ffmpeg::url::Url;
use jmap_vtc_server::MediaEffect;
use jmap_vtc_server::memory::MemoryBackend;
use jmap_vtc_server::{
    handle_vtc_livestream_get, handle_vtc_livestream_set, handle_vtc_recording_get,
    handle_vtc_recording_set,
};
use jmap_vtc_types::{VtcCapability, VtcLivestream, VtcRecording};
use serde_json::{Value, json};

pub const ACCOUNT: &str = "A1";
pub const BOB: &str = "user:bob@example.com";
pub const SYSTEM: &str = "service:media";
pub const BASE_URL: &str = "https://media.example.com/hls/";

/// A fresh directory under cargo's per-target scratch directory.
pub fn scratch() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "vtc-ffmpeg-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A configuration under a fresh scratch directory that polls every 5 ms
/// and gives up on a start after 2 s.
pub fn config() -> FfmpegConfig {
    let root = scratch();
    FfmpegConfig::new(
        root.join("hls"),
        Url::parse(BASE_URL).unwrap(),
        root.join("work"),
    )
    .with_poll_interval(Duration::from_millis(5))
    .with_start_timeout(Duration::from_secs(2))
    .with_stop_grace(Duration::from_millis(200))
}

/// What a fake process does.
#[derive(Debug, Default)]
pub struct Ctl {
    /// Set when the process has ended.
    pub exit: Option<ProcessExit>,
    pub stop_requested: bool,
    pub killed: bool,
    /// Ignore the stop request (so the adapter has to kill).
    pub hang: bool,
}

/// One recorded spawn.
#[derive(Debug, Clone)]
pub struct Spawned {
    pub args: Vec<String>,
    pub dir: PathBuf,
    pub ctl: Arc<Mutex<Ctl>>,
}

impl Spawned {
    /// End the process with exit status `code` (`None`: a signal).
    pub fn exit(&self, code: Option<i32>) {
        self.ctl.lock().unwrap().exit = Some(ProcessExit::new(code));
    }

    pub fn ctl(&self) -> std::sync::MutexGuard<'_, Ctl> {
        self.ctl.lock().unwrap()
    }

    /// The argument after `flag`.
    pub fn after(&self, flag: &str) -> &str {
        let i = self.args.iter().position(|a| a == flag).expect(flag);
        &self.args[i + 1]
    }
}

#[derive(Debug, Default)]
struct State {
    spawned: Vec<Spawned>,
    /// Write no output files (the process never goes live).
    silent: bool,
    /// Every spawn fails with `NotFound`.
    missing: bool,
    /// Started processes ignore the stop request.
    hang: bool,
    /// The concat run ends with this status.
    concat_exit: Option<i32>,
}

/// A [`ProcessRunner`] that records spawns and, like ffmpeg, writes the
/// HLS playlist, the progress file, the recording part and the joined
/// file into the working directory.
#[derive(Debug, Clone, Default)]
pub struct FakeRunner {
    state: Arc<Mutex<State>>,
}

impl FakeRunner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn silent(self) -> Self {
        self.state.lock().unwrap().silent = true;
        self
    }

    pub fn missing(self) -> Self {
        self.state.lock().unwrap().missing = true;
        self
    }

    pub fn hang(self) -> Self {
        self.state.lock().unwrap().hang = true;
        self
    }

    pub fn concat_exit(self, code: i32) -> Self {
        self.state.lock().unwrap().concat_exit = Some(code);
        self
    }

    pub fn spawned(&self) -> Vec<Spawned> {
        self.state.lock().unwrap().spawned.clone()
    }

    pub fn last(&self) -> Spawned {
        self.spawned().pop().expect("a spawn")
    }
}

impl ProcessRunner for FakeRunner {
    type Process = FakeProcess;

    fn spawn(&self, program: &Path, args: &[OsString], dir: &Path) -> io::Result<FakeProcess> {
        assert_eq!(program, Path::new("ffmpeg"));
        let mut state = self.state.lock().unwrap();
        if state.missing {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        let args: Vec<String> = args
            .iter()
            .map(|a| a.to_str().expect("utf-8 argument").to_owned())
            .collect();
        let ctl = Arc::new(Mutex::new(Ctl {
            hang: state.hang,
            ..Ctl::default()
        }));
        let last = args.last().expect("an output").clone();
        let has = |a: &str| args.iter().any(|x| x == a);
        if has("concat") {
            let code = state.concat_exit.unwrap_or(0);
            if code == 0 {
                let list = std::fs::read_to_string(dir.join("parts.txt")).unwrap();
                std::fs::write(dir.join(&last), format!("joined:\n{list}")).unwrap();
            }
            ctl.lock().unwrap().exit = Some(ProcessExit::new(Some(code)));
        } else if !state.silent {
            if has("hls") {
                std::fs::write(
                    dir.join(&last),
                    "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:2\n#EXTINF:2.000000,\nseg00000.ts\n",
                )
                .unwrap();
            } else if has("-progress") {
                std::fs::write(
                    dir.join("progress"),
                    "frame=1\ntotal_size=0\nprogress=continue\nframe=60\ntotal_size=4096\nprogress=continue\n",
                )
                .unwrap();
            } else if has("matroska") {
                std::fs::write(dir.join(&last), b"matroska").unwrap();
            }
        }
        state.spawned.push(Spawned {
            args,
            dir: dir.to_owned(),
            ctl: Arc::clone(&ctl),
        });
        Ok(FakeProcess { ctl })
    }
}

#[derive(Debug)]
pub struct FakeProcess {
    ctl: Arc<Mutex<Ctl>>,
}

impl MediaProcess for FakeProcess {
    fn try_wait(&mut self) -> io::Result<Option<ProcessExit>> {
        Ok(self.ctl.lock().unwrap().exit)
    }

    async fn wait(&mut self) -> io::Result<ProcessExit> {
        loop {
            if let Some(exit) = self.ctl.lock().unwrap().exit {
                return Ok(exit);
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }

    async fn request_stop(&mut self) -> io::Result<()> {
        let mut ctl = self.ctl.lock().unwrap();
        ctl.stop_requested = true;
        if !ctl.hang && ctl.exit.is_none() {
            ctl.exit = Some(ProcessExit::new(Some(0)));
        }
        Ok(())
    }

    async fn kill(&mut self) -> io::Result<()> {
        let mut ctl = self.ctl.lock().unwrap();
        ctl.killed = true;
        if ctl.exit.is_none() {
            ctl.exit = Some(ProcessExit::new(None));
        }
        Ok(())
    }
}

/// A backend whose account allows recordings (with pause) and hosted and
/// push livestreams over the schemes this crate supports, holding active
/// room call C1 started by BOB (its moderator).
pub fn backend() -> MemoryBackend {
    let b = MemoryBackend::new();
    let cap: VtcCapability = serde_json::from_value(json!({
        "mayCreateCall": true,
        "supportsRingCalls": true,
        "supportsRoomCalls": true,
        "supportsScheduledCalls": true,
        "supportsRecording": true,
        "supportsRecordingPause": true,
        "supportsLivestream": true,
        "livestreamModes": ["push", "hosted"],
        "livestreamPushSchemes": jmap_vtc_ffmpeg::args::PUSH_SCHEMES,
        "layouts": [],
        "supportsLobby": true,
        "supportsBreakoutRooms": true,
        "supportedMediaTypes": ["audio", "video"],
        "maxParticipantsPerCall": null,
        "maxConcurrentCalls": null,
        "maxBreakoutRooms": null
    }))
    .unwrap();
    b.register_account(&Id::from(ACCOUNT), cap);
    b.insert_object_for_test(
        "VTCCall",
        &Id::from(ACCOUNT),
        "C1",
        json!({
            "id": "C1", "accountId": ACCOUNT, "callType": "room", "state": "active",
            "createdAt": "2026-11-20T14:30:00Z", "endedAt": null, "endReason": null,
            "initiatorId": BOB, "subject": null,
            "joinUri": "https://meet.example.com/room/xkz", "joinPassword": null,
            "mediaTypes": ["audio", "video"], "activeParticipantCount": 0,
            "lobbyEnabled": false, "parentCallId": null, "breakoutRoomIds": [],
            "scheduledStartAt": null, "gatewayPin": null, "e2eeEnabled": false
        }),
    );
    b
}

pub fn bob() -> Id {
    Id::from(BOB)
}

pub fn system() -> Id {
    Id::from(SYSTEM)
}

pub async fn livestream_set(b: &MemoryBackend, mut args: Value) -> Value {
    args["accountId"] = json!(ACCOUNT);
    handle_vtc_livestream_set(b, &bob(), &BTreeMap::new(), args)
        .await
        .unwrap()
        .primary
}

pub async fn livestream_get(b: &MemoryBackend, id: &Id) -> Value {
    handle_vtc_livestream_get(b, &bob(), json!({"accountId": ACCOUNT, "ids": [id]}))
        .await
        .unwrap()
        .primary["list"][0]
        .clone()
}

pub async fn recording_set(b: &MemoryBackend, mut args: Value) -> Value {
    args["accountId"] = json!(ACCOUNT);
    handle_vtc_recording_set(b, &bob(), &BTreeMap::new(), args)
        .await
        .unwrap()
        .primary
}

pub async fn recording_get(b: &MemoryBackend, id: &Id) -> Value {
    handle_vtc_recording_get(b, &bob(), json!({"accountId": ACCOUNT, "ids": [id]}))
        .await
        .unwrap()
        .primary["list"][0]
        .clone()
}

/// The livestream of the last `StartLivestream` effect.
pub fn started_livestream(b: &MemoryBackend) -> VtcLivestream {
    b.media_effects_for_test()
        .into_iter()
        .rev()
        .find_map(|e| match e {
            MediaEffect::StartLivestream(ls) => Some(ls),
            _ => None,
        })
        .expect("a StartLivestream effect")
}

/// The recording of the last `StartRecording` effect.
pub fn started_recording(b: &MemoryBackend) -> VtcRecording {
    b.media_effects_for_test()
        .into_iter()
        .rev()
        .find_map(|e| match e {
            MediaEffect::StartRecording(r) => Some(r),
            _ => None,
        })
        .expect("a StartRecording effect")
}

pub fn last_effect(b: &MemoryBackend) -> Option<MediaEffect> {
    b.media_effects_for_test().pop()
}
