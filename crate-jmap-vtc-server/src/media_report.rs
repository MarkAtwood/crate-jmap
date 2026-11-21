//! Server-initiated `VTCLivestream` and `VTCRecording` transitions: what a
//! deployment's media pipeline reports back after
//! [`VtcBackend::run_media_effect`] started or stopped it.
//!
//! draft-atwood-jmap-vtc-00 makes these transitions the server's alone:
//! "`"starting"` → `"live"`: server-initiated", "`"starting"` or `"live"` →
//! `"failed"`: server-initiated, with `failureReason` set" (§Livestream
//! States), and for a recording "the server transitions the recording
//! through `"processing"` ... to `"available"` ..., or to `"failed"` on
//! error. These post-stop transitions are server-initiated and not
//! client-controllable" (§Pausing, Resuming, and Stopping a Recording).
//!
//! A [`MediaReporter`] applies each one through
//! [`VtcBackend::update_object_if`] with the draft's source states, so the
//! check is atomic with the write and the change advances the type's state
//! string and change log like any client update; a push of that state
//! change is the deployment's, as for every other write. The reports that
//! a client update also has (a recording paused, resumed or stopped)
//! deliver the same `VTCRecordingStateEvent`, and the ones that end a
//! livestream or recording request the same stop
//! [`MediaEffect`] a client stop does.
//!
//! Reports run outside any request, so no conditional Foo/set scope is
//! open, and each one commits as it returns.
//!
//! ```rust,no_run
//! # use jmap_types::Id;
//! # use jmap_vtc_server::VtcBackend;
//! # use jmap_vtc_server::media_report::MediaReporter;
//! # use jmap_vtc_types::{LatencyHint, PlaybackProtocol, VtcPlaybackEndpoint};
//! # async fn example<B: VtcBackend>(backend: &B, system: &B::CallerCtx) {
//! // The media pipeline says hosted livestream L1 now reaches its origin.
//! let reporter = MediaReporter::new(backend, system, Id::from("A1"));
//! let hls = VtcPlaybackEndpoint::new(
//!     PlaybackProtocol::Hls,
//!     "https://cdn.example.com/live/L1/index.m3u8",
//!     LatencyHint::Low,
//! );
//! reporter.livestream_live(&Id::from("L1"), vec![hls]).await.ok();
//! # }
//! ```

use std::fmt;

use jmap_server::{BackendSetError, GetObject, now_utc_string};
use jmap_types::{Id, PatchObject};
use jmap_vtc_types::{
    LivestreamFailureReason, LivestreamMode, LivestreamState, RecordingState, VtcLivestream,
    VtcPlaybackEndpoint, VtcRecording,
};
use serde_json::json;

use crate::media::{deliver, stopped_livestream};
use crate::{MediaEffect, StatefulObject, VtcBackend};

/// Why a report changed nothing.
#[non_exhaustive]
#[derive(Debug)]
pub enum ReportError<E, S> {
    /// No such livestream or recording in the account.
    NotFound,
    /// The stored `state` is not one the transition starts from, checked
    /// atomically with the write. The value is the stored state. A report
    /// that races a moderator's stop ends here, which is the draft's
    /// outcome: "`"stopped"` and `"failed"` are final".
    WrongState(S),
    /// The report breaks a draft rule whatever the state, for example
    /// playback endpoints for a push-mode livestream ("Always empty in push
    /// mode").
    Invalid(&'static str),
    /// The backend failed.
    Backend(BackendSetError<E>),
}

impl<E: fmt::Display, S: fmt::Debug> fmt::Display for ReportError<E, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("no such object"),
            Self::WrongState(s) => write!(f, "the transition does not start from state {s:?}"),
            Self::Invalid(why) => write!(f, "invalid report: {why}"),
            Self::Backend(e) => write!(f, "backend: {e}"),
        }
    }
}

impl<E, S> std::error::Error for ReportError<E, S>
where
    E: std::error::Error + 'static,
    S: fmt::Debug,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Backend(e) => Some(e),
            _ => None,
        }
    }
}

/// The result of a livestream report.
pub type LivestreamReport<E> = Result<(), ReportError<E, LivestreamState>>;

/// The result of a recording report.
pub type RecordingReport<E> = Result<(), ReportError<E, RecordingState>>;

/// The finished file of a recording, for [`MediaReporter::recording_available`]:
/// "`"available"` (when `blobId`, `size`, `duration`, and `mediaType` are
/// set)".
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingFile {
    /// The JMAP blob holding the file; the backend has stored it.
    pub blob_id: Id,
    /// The file size in bytes.
    pub size: u64,
    /// The recording duration in seconds.
    pub duration: u64,
    /// The file's MIME type, e.g. `"video/mp4"`.
    pub media_type: String,
}

impl RecordingFile {
    /// A recording file: blob, size in bytes, duration in seconds and MIME
    /// type.
    pub fn new(blob_id: Id, size: u64, duration: u64, media_type: impl Into<String>) -> Self {
        Self {
            blob_id,
            size,
            duration,
            media_type: media_type.into(),
        }
    }
}

/// Applies server-initiated media transitions to one account.
///
/// `caller` is the deployment's own context for these writes (a service
/// identity): the reports pass it to every backend call, and the
/// `VTCRecordingStateEvent`s they deliver name its
/// [`VtcBackend::caller_user_id`] as `initiatedBy`. Every read goes through
/// [`VtcBackend::get_objects_unscoped`], so §Per-call access does not hide
/// the object from the reporter.
pub struct MediaReporter<'a, B: VtcBackend> {
    backend: &'a B,
    caller: &'a B::CallerCtx,
    account_id: Id,
}

impl<B: VtcBackend> fmt::Debug for MediaReporter<'_, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MediaReporter")
            .field("account_id", &self.account_id)
            .finish_non_exhaustive()
    }
}

impl<'a, B: VtcBackend> MediaReporter<'a, B> {
    /// A reporter writing to `account_id` of `backend` as `caller`.
    pub fn new(backend: &'a B, caller: &'a B::CallerCtx, account_id: Id) -> Self {
        Self {
            backend,
            caller,
            account_id,
        }
    }

    /// The livestream is reaching its destination (push mode) or its
    /// playback endpoints (hosted mode): `"starting"` → `"live"`, with
    /// `startedAt` now ("Time the stream went live") and
    /// `playbackEndpoints` set to `playback_endpoints`.
    ///
    /// # Errors
    ///
    /// [`ReportError::Invalid`] for endpoints on a push-mode livestream
    /// ("Always empty in push mode"), [`ReportError::WrongState`] unless it
    /// is `"starting"`, [`ReportError::NotFound`] and
    /// [`ReportError::Backend`].
    pub async fn livestream_live(
        &self,
        id: &Id,
        playback_endpoints: Vec<VtcPlaybackEndpoint>,
    ) -> LivestreamReport<B::Error> {
        let livestream: VtcLivestream = self.fetch(id).await?;
        check_endpoints(&livestream, &playback_endpoints)?;
        let mut patch = PatchObject::new();
        let map = patch.as_map_mut();
        map.insert("state".to_owned(), json!(LivestreamState::Live));
        map.insert("startedAt".to_owned(), json!(now_utc_string()));
        map.insert("playbackEndpoints".to_owned(), json!(playback_endpoints));
        self.apply::<VtcLivestream>(id, &[LivestreamState::Starting], patch)
            .await
    }

    /// The hosted livestream's playback endpoints changed: they became
    /// available while `"starting"` ("The list MAY be empty while the
    /// livestream is `"starting"`"), or the endpoint set or a
    /// `timeReference` changed while `"live"` ("The server updates
    /// `timeReference` when the mapping changes ...; this is an ordinary
    /// change to the `VTCLivestream`"). Replaces the stored list.
    ///
    /// # Errors
    ///
    /// As for [`Self::livestream_live`], with `"starting"` or `"live"` as
    /// the source states.
    pub async fn livestream_endpoints(
        &self,
        id: &Id,
        playback_endpoints: Vec<VtcPlaybackEndpoint>,
    ) -> LivestreamReport<B::Error> {
        let livestream: VtcLivestream = self.fetch(id).await?;
        check_endpoints(&livestream, &playback_endpoints)?;
        let mut patch = PatchObject::new();
        patch
            .as_map_mut()
            .insert("playbackEndpoints".to_owned(), json!(playback_endpoints));
        self.apply::<VtcLivestream>(
            id,
            &[LivestreamState::Starting, LivestreamState::Live],
            patch,
        )
        .await
    }

    /// The livestream failed: `"starting"` or `"live"` → `"failed"` with
    /// `failureReason` `reason`, `stoppedAt` now ("Time the stream stopped
    /// or failed") and no playback endpoints ("empty once it is `"stopped"`
    /// or `"failed"`"). Then requests [`MediaEffect::StopLivestream`], so the
    /// backend releases the pipeline through the same hook as for a stop.
    ///
    /// # Errors
    ///
    /// [`ReportError::WrongState`] unless it is `"starting"` or `"live"`,
    /// [`ReportError::NotFound`] and [`ReportError::Backend`].
    pub async fn livestream_failed(
        &self,
        id: &Id,
        reason: LivestreamFailureReason,
    ) -> LivestreamReport<B::Error> {
        let livestream: VtcLivestream = self.fetch(id).await?;
        let stopped_at = now_utc_string();
        let mut patch = PatchObject::new();
        let map = patch.as_map_mut();
        map.insert("state".to_owned(), json!(LivestreamState::Failed));
        map.insert("failureReason".to_owned(), json!(reason));
        map.insert("stoppedAt".to_owned(), json!(stopped_at));
        map.insert("playbackEndpoints".to_owned(), json!([]));
        self.apply::<VtcLivestream>(
            id,
            &[LivestreamState::Starting, LivestreamState::Live],
            patch,
        )
        .await?;
        let mut failed = stopped_livestream(&livestream, stopped_at);
        failed.state = LivestreamState::Failed;
        failed.failure_reason = Some(reason);
        self.effect(MediaEffect::StopLivestream(failed)).await;
        Ok(())
    }

    /// The server stopped the livestream on its own, for example the idle
    /// stop of §Livestream States ("the server MAY stop a livestream, as if
    /// a moderator had set it to `"stopped"`, once it has carried no media
    /// from the call for a deployment-defined period"): `"starting"` or
    /// `"live"` → `"stopped"`, as a moderator's stop, then
    /// [`MediaEffect::StopLivestream`].
    ///
    /// The draft reserves `"stopped"` for a moderator, the call's end and
    /// the idle stop: "When a livestream ends for any reason other than
    /// those listed for `"stopped"`, the server SHOULD move it to
    /// `"failed"`". Report a lost stream with [`Self::livestream_failed`].
    ///
    /// # Errors
    ///
    /// As for [`Self::livestream_failed`].
    pub async fn livestream_stopped(&self, id: &Id) -> LivestreamReport<B::Error> {
        let livestream: VtcLivestream = self.fetch(id).await?;
        let stopped_at = now_utc_string();
        let mut patch = PatchObject::new();
        let map = patch.as_map_mut();
        map.insert("state".to_owned(), json!(LivestreamState::Stopped));
        map.insert("stoppedAt".to_owned(), json!(stopped_at));
        map.insert("playbackEndpoints".to_owned(), json!([]));
        self.apply::<VtcLivestream>(
            id,
            &[LivestreamState::Starting, LivestreamState::Live],
            patch,
        )
        .await?;
        self.effect(MediaEffect::StopLivestream(stopped_livestream(
            &livestream,
            stopped_at,
        )))
        .await;
        Ok(())
    }

    /// The pipeline paused the recording: `"recording"` → `"paused"`, with
    /// the `VTCRecordingStateEvent` a moderator's pause delivers ("The
    /// server delivers a `VTCRecordingStateEvent` with `state:
    /// "paused"`"). No media effect: the pipeline has already paused.
    ///
    /// # Errors
    ///
    /// [`ReportError::WrongState`] unless it is `"recording"`,
    /// [`ReportError::NotFound`] and [`ReportError::Backend`].
    pub async fn recording_paused(&self, id: &Id) -> RecordingReport<B::Error> {
        self.recording_state(id, RecordingState::Paused, &[RecordingState::Recording])
            .await
    }

    /// The pipeline resumed the recording: `"paused"` → `"recording"`, with
    /// the `VTCRecordingStateEvent`. No media effect.
    ///
    /// # Errors
    ///
    /// [`ReportError::WrongState`] unless it is `"paused"`,
    /// [`ReportError::NotFound`] and [`ReportError::Backend`].
    pub async fn recording_resumed(&self, id: &Id) -> RecordingReport<B::Error> {
        self.recording_state(id, RecordingState::Recording, &[RecordingState::Paused])
            .await
    }

    /// The recording stopped without a moderator (the pipeline ended it,
    /// or a deployment limit did): `"recording"` or `"paused"` →
    /// `"stopped"` with `stoppedAt` now and the `VTCRecordingStateEvent`
    /// with `state: "stopped"`, then [`MediaEffect::StopRecording`], as for a
    /// moderator's stop. Follow with [`Self::recording_processing`] and
    /// [`Self::recording_available`] or [`Self::recording_failed`].
    ///
    /// # Errors
    ///
    /// [`ReportError::WrongState`] unless it is `"recording"` or
    /// `"paused"`, [`ReportError::NotFound`] and [`ReportError::Backend`].
    pub async fn recording_stopped(&self, id: &Id) -> RecordingReport<B::Error> {
        let recording: VtcRecording = self.fetch(id).await?;
        let stopped_at = now_utc_string();
        let mut patch = PatchObject::new();
        let map = patch.as_map_mut();
        map.insert("state".to_owned(), json!(RecordingState::Stopped));
        map.insert("stoppedAt".to_owned(), json!(stopped_at));
        self.apply::<VtcRecording>(
            id,
            &[RecordingState::Recording, RecordingState::Paused],
            patch,
        )
        .await?;
        self.event(&recording, RecordingState::Stopped).await?;
        let mut stopped = recording;
        stopped.state = RecordingState::Stopped;
        stopped.stopped_at = Some(stopped_at);
        self.effect(MediaEffect::StopRecording(stopped)).await;
        Ok(())
    }

    /// The recording file is being prepared: `"stopped"` → `"processing"`,
    /// with `duration` in seconds when known ("Available only when `state`
    /// is `"stopped"`, `"processing"`, or `"available"`").
    ///
    /// # Errors
    ///
    /// [`ReportError::WrongState`] unless it is `"stopped"`,
    /// [`ReportError::NotFound`] and [`ReportError::Backend`].
    pub async fn recording_processing(
        &self,
        id: &Id,
        duration: Option<u64>,
    ) -> RecordingReport<B::Error> {
        let mut patch = PatchObject::new();
        let map = patch.as_map_mut();
        map.insert("state".to_owned(), json!(RecordingState::Processing));
        if let Some(duration) = duration {
            map.insert("duration".to_owned(), json!(duration));
        }
        self.apply::<VtcRecording>(id, &[RecordingState::Stopped], patch)
            .await
    }

    /// The recording file is ready: `"stopped"` or `"processing"` →
    /// `"available"`, with `blobId`, `size`, `duration` and `mediaType`
    /// from `file`.
    ///
    /// # Errors
    ///
    /// [`ReportError::WrongState`] unless it is `"stopped"` or
    /// `"processing"`, [`ReportError::NotFound`] and
    /// [`ReportError::Backend`].
    pub async fn recording_available(
        &self,
        id: &Id,
        file: RecordingFile,
    ) -> RecordingReport<B::Error> {
        let mut patch = PatchObject::new();
        let map = patch.as_map_mut();
        map.insert("state".to_owned(), json!(RecordingState::Available));
        map.insert("blobId".to_owned(), json!(file.blob_id));
        map.insert("size".to_owned(), json!(file.size));
        map.insert("duration".to_owned(), json!(file.duration));
        map.insert("mediaType".to_owned(), json!(file.media_type));
        self.apply::<VtcRecording>(
            id,
            &[RecordingState::Stopped, RecordingState::Processing],
            patch,
        )
        .await
    }

    /// Preparing the file failed: `"stopped"` or `"processing"` →
    /// `"failed"` ("or to `"failed"` on error"). A recording that fails
    /// while still recording is first reported with
    /// [`Self::recording_stopped`].
    ///
    /// # Errors
    ///
    /// As for [`Self::recording_available`].
    pub async fn recording_failed(&self, id: &Id) -> RecordingReport<B::Error> {
        let mut patch = PatchObject::new();
        patch
            .as_map_mut()
            .insert("state".to_owned(), json!(RecordingState::Failed));
        self.apply::<VtcRecording>(
            id,
            &[RecordingState::Stopped, RecordingState::Processing],
            patch,
        )
        .await
    }

    /// A pause or resume: the state change and its event.
    async fn recording_state(
        &self,
        id: &Id,
        to: RecordingState,
        from: &[RecordingState],
    ) -> RecordingReport<B::Error> {
        let recording: VtcRecording = self.fetch(id).await?;
        let mut patch = PatchObject::new();
        patch.as_map_mut().insert("state".to_owned(), json!(to));
        self.apply::<VtcRecording>(id, from, patch).await?;
        self.event(&recording, to).await
    }

    /// Deliver the `VTCRecordingStateEvent` for `recording` now in `state`.
    async fn event(
        &self,
        recording: &VtcRecording,
        state: RecordingState,
    ) -> RecordingReport<B::Error> {
        let initiated_by = self
            .backend
            .caller_user_id(self.caller, &self.account_id)
            .await
            .map_err(|e| ReportError::Backend(BackendSetError::Other(e)))?;
        deliver(
            self.backend,
            self.caller,
            &self.account_id,
            recording.call_id.clone(),
            recording.id.clone(),
            state,
            initiated_by,
        )
        .await;
        Ok(())
    }

    async fn effect(&self, effect: MediaEffect) {
        self.backend
            .run_media_effect(self.caller, &self.account_id, effect)
            .await;
    }

    /// The stored object `id`, read without §Per-call access.
    async fn fetch<O, S>(&self, id: &Id) -> Result<O, ReportError<B::Error, S>>
    where
        O: GetObject + Send + Sync,
    {
        let (mut found, _) = self
            .backend
            .get_objects_unscoped::<O>(
                self.caller,
                &self.account_id,
                Some(std::slice::from_ref(id)),
            )
            .await
            .map_err(|e| ReportError::Backend(BackendSetError::Other(e)))?;
        found.pop().ok_or(ReportError::NotFound)
    }

    /// `patch` on `id` if its stored state is one of `from`.
    async fn apply<O>(
        &self,
        id: &Id,
        from: &[O::State],
        patch: PatchObject,
    ) -> Result<(), ReportError<B::Error, O::State>>
    where
        O: StatefulObject<Patch = PatchObject> + Send + Sync,
    {
        match self
            .backend
            .update_object_if::<O>(self.caller, &self.account_id, id, from, patch)
            .await
        {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(state)) => Err(ReportError::WrongState(state)),
            Err(BackendSetError::SetError(e))
                if e.error_type == jmap_server::SetErrorType::NotFound =>
            {
                Err(ReportError::NotFound)
            }
            Err(e) => Err(ReportError::Backend(e)),
        }
    }
}

/// §`VTCLivestream` `playbackEndpoints`: "Always empty in push mode".
fn check_endpoints<E>(
    livestream: &VtcLivestream,
    endpoints: &[VtcPlaybackEndpoint],
) -> Result<(), ReportError<E, LivestreamState>> {
    if livestream.mode != LivestreamMode::Hosted && !endpoints.is_empty() {
        return Err(ReportError::Invalid(
            "a push-mode livestream has no playback endpoints",
        ));
    }
    Ok(())
}
