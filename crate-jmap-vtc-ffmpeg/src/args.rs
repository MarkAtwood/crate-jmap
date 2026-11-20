//! ffmpeg argument vectors, built by pure functions.
//!
//! Every function here returns the arguments after the program name, as
//! one `OsString` per argument, for [`ProcessRunner::spawn`](crate::process::ProcessRunner::spawn).
//! Nothing goes through a shell. Output files are named relative to the
//! process's working directory (the livestream's or recording's own
//! directory), so a configured root path never appears in an argument
//! where ffmpeg would read `%` as a pattern.
//!
//! Oracles: the ffmpeg documentation, "ffmpeg Documentation"
//! (<https://ffmpeg.org/ffmpeg.html>: `-re`, `-progress`,
//! `-stats_period`, `-map`), "FFmpeg Formats Documentation"
//! (<https://ffmpeg.org/ffmpeg-formats.html>: the `hls`, `concat`,
//! `matroska`, `mp4` (`movflags`), `flv`, `mpegts` and `whip` muxers and
//! demuxers) and "FFmpeg Filters Documentation"
//! (<https://ffmpeg.org/ffmpeg-filters.html>: the `testsrc2` and `sine`
//! sources).

use std::ffi::OsString;
use std::fmt;

use url::Url;

use crate::FfmpegError;

/// The HLS playlist a hosted livestream writes in its directory.
pub const PLAYLIST: &str = "index.m3u8";

/// The HLS segment file pattern (`seg00000.ts`, `seg00001.ts`, ...).
pub const SEGMENT_PATTERN: &str = "seg%05d.ts";

/// The `-progress` file of a push livestream.
pub const PROGRESS: &str = "progress";

/// The concat demuxer list of a stopped recording.
pub const CONCAT_LIST: &str = "parts.txt";

/// The finished recording file.
pub const RECORDING_FILE: &str = "recording.mp4";

/// The MIME type of [`RECORDING_FILE`].
pub const RECORDING_MEDIA_TYPE: &str = "video/mp4";

/// The URI schemes [`PushTarget::new`] accepts, for the account's
/// `livestreamPushSchemes`. `"https"` is a WHIP endpoint and needs an
/// ffmpeg with the `whip` muxer (8.0 or later).
pub const PUSH_SCHEMES: &[&str] = &["rtmp", "rtmps", "srt", "https"];

/// The file of recording part `n` (1-based): `part-0001.mkv`. Matroska
/// stays readable up to the last written cluster when ffmpeg is killed.
pub fn part_name(n: u32) -> String {
    format!("part-{n:04}.mkv")
}

/// Where the media comes from.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// ffmpeg's synthetic `testsrc2` picture and `sine` tone: no camera,
    /// no SFU, no network. For tests and smoke runs.
    TestPattern(TestPattern),
    /// Any ffmpeg input: the call's composited feed, an SDP file for an
    /// SFU's RTP forward, an SRT or RTMP listener, a file.
    Input(InputSource),
}

impl Source {
    /// The default [`TestPattern`].
    pub fn test_pattern() -> Self {
        Self::TestPattern(TestPattern::default())
    }

    /// An [`InputSource`] reading `url`.
    pub fn input(url: impl Into<OsString>) -> Self {
        Self::Input(InputSource::new(url))
    }
}

/// The synthetic source: `-f lavfi -i testsrc2=...` for video and
/// `-f lavfi -i sine=...` for audio, both read at native rate (`-re`) so
/// the output runs in real time.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestPattern {
    /// Picture width in pixels.
    pub width: u32,
    /// Picture height in pixels.
    pub height: u32,
    /// Frames per second.
    pub frame_rate: u32,
    /// Tone frequency in Hz.
    pub tone_hz: u32,
}

impl Default for TestPattern {
    /// 1280x720 at 30 frames per second with a 1 kHz tone.
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            frame_rate: 30,
            tone_hz: 1000,
        }
    }
}

impl TestPattern {
    /// This pattern at `width` x `height`.
    pub fn with_size(self, width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            ..self
        }
    }

    /// This pattern at `frame_rate` frames per second.
    pub fn with_frame_rate(self, frame_rate: u32) -> Self {
        Self { frame_rate, ..self }
    }
}

/// An ffmpeg input: `[-re] <options...> -i <url>`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputSource {
    /// Input options placed before `-i`, e.g. `-protocol_whitelist
    /// file,udp,rtp` for an SDP file.
    pub options: Vec<OsString>,
    /// The input URL or path.
    pub url: OsString,
    /// Read at native frame rate (`-re`): for a file or other input that
    /// is not live already.
    pub realtime: bool,
}

impl InputSource {
    /// Read `url`, live (no `-re`), with no options.
    pub fn new(url: impl Into<OsString>) -> Self {
        Self {
            options: Vec::new(),
            url: url.into(),
            realtime: false,
        }
    }

    /// This input with `options` before `-i`.
    pub fn with_options<I, S>(self, options: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        Self {
            options: options.into_iter().map(Into::into).collect(),
            ..self
        }
    }

    /// This input read at native frame rate (`-re`).
    pub fn realtime(self) -> Self {
        Self {
            realtime: true,
            ..self
        }
    }
}

/// How the media is encoded: H.264 and AAC by default, which HLS players,
/// RTMP ingest and MP4 all take.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoding {
    /// The `-c:v` encoder.
    pub video_codec: String,
    /// The `-preset`; `None` for none.
    pub preset: Option<String>,
    /// The video bitrate in kbit/s (`-b:v`, `-maxrate`, and twice it as
    /// `-bufsize`).
    pub video_bitrate_kbps: u32,
    /// The output frame rate (`-r`).
    pub frame_rate: u32,
    /// Seconds between keyframes (`-g` and `-keyint_min` in frames, with
    /// scene-cut keyframes off), so HLS segments cut evenly.
    pub keyframe_interval_secs: u32,
    /// The `-c:a` encoder.
    pub audio_codec: String,
    /// The audio bitrate in kbit/s (`-b:a`).
    pub audio_bitrate_kbps: u32,
}

impl Default for Encoding {
    /// `libx264` `veryfast` (with `-tune zerolatency`) at 2500 kbit/s and
    /// 30 frames per second, a keyframe every 2 s, AAC at 128 kbit/s.
    fn default() -> Self {
        Self {
            video_codec: "libx264".to_owned(),
            preset: Some("veryfast".to_owned()),
            video_bitrate_kbps: 2500,
            frame_rate: 30,
            keyframe_interval_secs: 2,
            audio_codec: "aac".to_owned(),
            audio_bitrate_kbps: 128,
        }
    }
}

impl Encoding {
    /// This encoding at `kbps` kbit/s of video.
    pub fn with_video_bitrate_kbps(self, kbps: u32) -> Self {
        Self {
            video_bitrate_kbps: kbps,
            ..self
        }
    }

    /// This encoding at `frame_rate` frames per second.
    pub fn with_frame_rate(self, frame_rate: u32) -> Self {
        Self { frame_rate, ..self }
    }
}

/// The HLS muxer settings of a hosted livestream.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HlsSettings {
    /// Target segment length in seconds (`-hls_time`).
    pub segment_secs: u32,
    /// Segments kept in the playlist (`-hls_list_size`).
    pub list_size: u32,
    /// Delete segments that left the playlist (`delete_segments`).
    pub delete_segments: bool,
}

impl Default for HlsSettings {
    /// 2 s segments, 6 in the playlist, old ones deleted.
    fn default() -> Self {
        Self {
            segment_secs: 2,
            list_size: 6,
            delete_segments: true,
        }
    }
}

impl HlsSettings {
    /// These settings with `segment_secs` second segments.
    pub fn with_segment_secs(self, segment_secs: u32) -> Self {
        Self {
            segment_secs,
            ..self
        }
    }

    /// These settings keeping `list_size` segments.
    pub fn with_list_size(self, list_size: u32) -> Self {
        Self { list_size, ..self }
    }

    /// These settings keeping every segment, for a playlist that is
    /// replayable from the start after the livestream ends.
    pub fn keep_segments(self) -> Self {
        Self {
            delete_segments: false,
            ..self
        }
    }
}

/// A push-mode destination: the livestream's `streamUri` with its
/// `streamKey` presented as draft-atwood-jmap-vtc-00 §Presenting the
/// Stream Key requires.
///
/// The output URL (and for WHIP the token) carries the stream key, so it
/// is an ingest credential: the `Debug` impl shows only the scheme and
/// host, and the arguments [`push_args`] builds must not be logged.
#[derive(Clone, PartialEq, Eq)]
pub struct PushTarget {
    kind: PushKind,
    url: String,
    host: String,
    token: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PushKind {
    Rtmp,
    Srt,
    Whip,
}

impl PushTarget {
    /// The destination for `stream_uri` and `stream_key`:
    ///
    /// | Scheme | ffmpeg output | Stream key |
    /// |---|---|---|
    /// | `rtmp`, `rtmps` | `-f flv` | "the RTMP stream name, appended to the application path given by `streamUri`" |
    /// | `srt` | `-f mpegts` | "the SRT stream ID", the `streamid` query parameter; "A `streamUri` that already carries a stream ID is used as given" |
    /// | `https` | `-f whip` | "the bearer token with which the WHIP client authenticates" (`-authorization`) |
    ///
    /// The scheme is matched case-insensitively (RFC 3986 §3.1).
    ///
    /// # Errors
    ///
    /// [`FfmpegError::InvalidStreamUri`] for a `stream_uri` that is not an
    /// absolute URI with a host, [`FfmpegError::UnsupportedScheme`] for a
    /// scheme not in [`PUSH_SCHEMES`].
    pub fn new(stream_uri: &str, stream_key: Option<&str>) -> Result<Self, FfmpegError> {
        let mut url = Url::parse(stream_uri).map_err(|_| FfmpegError::InvalidStreamUri)?;
        if !PUSH_SCHEMES.contains(&url.scheme()) {
            return Err(FfmpegError::UnsupportedScheme(url.scheme().to_owned()));
        }
        let host = url
            .host_str()
            .filter(|h| !h.is_empty())
            .ok_or(FfmpegError::InvalidStreamUri)?
            .to_owned();
        let key = stream_key.filter(|k| !k.is_empty());
        let (kind, token) = match url.scheme() {
            "rtmp" | "rtmps" => {
                if let Some(key) = key {
                    url.path_segments_mut()
                        .map_err(|()| FfmpegError::InvalidStreamUri)?
                        .pop_if_empty()
                        .push(key);
                }
                (PushKind::Rtmp, None)
            }
            "srt" => {
                let has_id = url.query_pairs().any(|(k, _)| k == "streamid");
                if let (Some(key), false) = (key, has_id) {
                    url.query_pairs_mut().append_pair("streamid", key);
                }
                (PushKind::Srt, None)
            }
            "https" => (PushKind::Whip, key.map(str::to_owned)),
            other => return Err(FfmpegError::UnsupportedScheme(other.to_owned())),
        };
        Ok(Self {
            kind,
            url: url.into(),
            host,
            token,
        })
    }

    /// The scheme family: `"rtmp"`, `"srt"` or `"https"`.
    pub fn scheme(&self) -> &'static str {
        match self.kind {
            PushKind::Rtmp => "rtmp",
            PushKind::Srt => "srt",
            PushKind::Whip => "https",
        }
    }

    /// The destination host.
    pub fn host(&self) -> &str {
        &self.host
    }
}

impl fmt::Debug for PushTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PushTarget")
            .field("scheme", &self.scheme())
            .field("host", &self.host)
            .finish_non_exhaustive()
    }
}

fn push(args: &mut Vec<OsString>, items: &[&str]) {
    args.extend(items.iter().map(OsString::from));
}

/// The options every invocation starts with: no banner, no stats line,
/// warnings and errors only, overwrite outputs.
fn common() -> Vec<OsString> {
    let mut args = Vec::new();
    push(
        &mut args,
        &["-hide_banner", "-nostats", "-loglevel", "warning", "-y"],
    );
    args
}

/// The input arguments of `source`, with the `-map`s that pick its video
/// and audio.
pub fn input_args(source: &Source) -> Vec<OsString> {
    let mut args = Vec::new();
    match source {
        Source::TestPattern(p) => {
            push(&mut args, &["-re", "-f", "lavfi", "-i"]);
            args.push(
                format!(
                    "testsrc2=size={}x{}:rate={}",
                    p.width, p.height, p.frame_rate
                )
                .into(),
            );
            push(&mut args, &["-re", "-f", "lavfi", "-i"]);
            args.push(format!("sine=frequency={}:sample_rate=48000", p.tone_hz).into());
            push(&mut args, &["-map", "0:v:0", "-map", "1:a:0"]);
        }
        Source::Input(input) => {
            if input.realtime {
                push(&mut args, &["-re"]);
            }
            args.extend(input.options.iter().cloned());
            push(&mut args, &["-i"]);
            args.push(input.url.clone());
        }
    }
    args
}

/// The encoder arguments of `encoding`. `-tune zerolatency` only goes
/// with `libx264`, the one encoder that has it.
pub fn encode_args(encoding: &Encoding) -> Vec<OsString> {
    let mut args = Vec::new();
    push(&mut args, &["-c:v", &encoding.video_codec]);
    if let Some(preset) = &encoding.preset {
        push(&mut args, &["-preset", preset]);
    }
    if encoding.video_codec == "libx264" {
        push(&mut args, &["-tune", "zerolatency"]);
    }
    let gop = (encoding.frame_rate * encoding.keyframe_interval_secs).max(1);
    let rate = encoding.video_bitrate_kbps;
    push(
        &mut args,
        &[
            "-pix_fmt",
            "yuv420p",
            "-r",
            &encoding.frame_rate.to_string(),
            "-g",
            &gop.to_string(),
            "-keyint_min",
            &gop.to_string(),
            "-sc_threshold",
            "0",
            "-b:v",
            &format!("{rate}k"),
            "-maxrate",
            &format!("{rate}k"),
            "-bufsize",
            &format!("{}k", rate * 2),
            "-c:a",
            &encoding.audio_codec,
            "-b:a",
            &format!("{}k", encoding.audio_bitrate_kbps),
            "-ar",
            "48000",
            "-ac",
            "2",
        ],
    );
    args
}

/// A hosted livestream: `source` encoded with `encoding` into an HLS
/// playlist [`PLAYLIST`] with segments [`SEGMENT_PATTERN`] in the working
/// directory, each tagged with `EXT-X-PROGRAM-DATE-TIME`. ffmpeg writes
/// the playlist after the first segment is complete, and `EXT-X-ENDLIST`
/// when it ends cleanly.
pub fn hls_args(source: &Source, encoding: &Encoding, hls: &HlsSettings) -> Vec<OsString> {
    let mut args = common();
    args.extend(input_args(source));
    args.extend(encode_args(encoding));
    let mut flags = String::from("independent_segments+program_date_time");
    if hls.delete_segments {
        flags.push_str("+delete_segments");
    }
    push(
        &mut args,
        &[
            "-f",
            "hls",
            "-hls_time",
            &hls.segment_secs.to_string(),
            "-hls_list_size",
            &hls.list_size.to_string(),
            "-hls_flags",
            &flags,
            "-hls_segment_filename",
            SEGMENT_PATTERN,
            PLAYLIST,
        ],
    );
    args
}

/// A push livestream: `source` encoded with `encoding` and sent to
/// `target`, with progress written every 2 s to [`PROGRESS`] in the
/// working directory (its `total_size` turns positive once the output
/// has taken bytes).
///
/// The result carries the stream key; do not log it.
pub fn push_args(source: &Source, encoding: &Encoding, target: &PushTarget) -> Vec<OsString> {
    let mut args = common();
    push(&mut args, &["-progress", PROGRESS, "-stats_period", "2"]);
    args.extend(input_args(source));
    args.extend(encode_args(encoding));
    match target.kind {
        PushKind::Rtmp => push(&mut args, &["-f", "flv"]),
        PushKind::Srt => push(&mut args, &["-f", "mpegts"]),
        PushKind::Whip => {
            push(&mut args, &["-f", "whip"]);
            if let Some(token) = &target.token {
                push(&mut args, &["-authorization", token]);
            }
        }
    }
    args.push(target.url.clone().into());
    args
}

/// One recording part: `source` encoded with `encoding` into the Matroska
/// file `part` in the working directory (see [`part_name`]).
pub fn record_args(source: &Source, encoding: &Encoding, part: &str) -> Vec<OsString> {
    let mut args = common();
    args.extend(input_args(source));
    args.extend(encode_args(encoding));
    push(&mut args, &["-f", "matroska", part]);
    args
}

/// Join the parts listed in [`CONCAT_LIST`] into [`RECORDING_FILE`]
/// without re-encoding, the moov atom at the front (`+faststart`) so the
/// file plays while it downloads.
pub fn concat_args() -> Vec<OsString> {
    let mut args = common();
    push(
        &mut args,
        &[
            "-f",
            "concat",
            "-i",
            CONCAT_LIST,
            "-c",
            "copy",
            "-movflags",
            "+faststart",
            RECORDING_FILE,
        ],
    );
    args
}

/// The [`CONCAT_LIST`] content for `parts`, names relative to the list's
/// own directory (the concat demuxer's safe mode accepts them): one
/// `file '<name>'` line per part, a `'` in a name written `'\''`.
pub fn concat_list<S: AsRef<str>>(parts: &[S]) -> String {
    parts
        .iter()
        .map(|p| format!("file '{}'\n", p.as_ref().replace('\'', r"'\''")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[OsString]) -> Vec<&str> {
        args.iter().map(|a| a.to_str().expect("utf-8")).collect()
    }

    /// The argument after `flag`.
    fn after<'a>(args: &[&'a str], flag: &str) -> &'a str {
        let i = args.iter().position(|a| *a == flag).expect(flag);
        args[i + 1]
    }

    #[test]
    fn test_pattern_input() {
        let args = input_args(&Source::test_pattern());
        assert_eq!(
            strings(&args),
            [
                "-re",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=1280x720:rate=30",
                "-re",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=1000:sample_rate=48000",
                "-map",
                "0:v:0",
                "-map",
                "1:a:0",
            ]
        );
    }

    #[test]
    fn external_input_keeps_options_before_i() {
        let source = Source::Input(
            InputSource::new("call.sdp")
                .with_options(["-protocol_whitelist", "file,udp,rtp"])
                .realtime(),
        );
        assert_eq!(
            strings(&input_args(&source)),
            [
                "-re",
                "-protocol_whitelist",
                "file,udp,rtp",
                "-i",
                "call.sdp"
            ]
        );
        assert_eq!(
            strings(&input_args(&Source::input(
                "srt://0.0.0.0:9000?mode=listener"
            ))),
            ["-i", "srt://0.0.0.0:9000?mode=listener"]
        );
    }

    #[test]
    fn default_encoding() {
        let args = encode_args(&Encoding::default());
        let s = strings(&args);
        assert_eq!(after(&s, "-c:v"), "libx264");
        assert_eq!(after(&s, "-tune"), "zerolatency");
        // A keyframe every 2 s at 30 fps.
        assert_eq!(after(&s, "-g"), "60");
        assert_eq!(after(&s, "-keyint_min"), "60");
        assert_eq!(after(&s, "-b:v"), "2500k");
        assert_eq!(after(&s, "-bufsize"), "5000k");
        assert_eq!(after(&s, "-c:a"), "aac");
        assert_eq!(after(&s, "-b:a"), "128k");

        let other = Encoding {
            video_codec: "libopenh264".to_owned(),
            preset: None,
            ..Encoding::default()
        };
        let s = encode_args(&other);
        let s = strings(&s);
        assert!(!s.contains(&"-tune"), "{s:?}");
        assert!(!s.contains(&"-preset"), "{s:?}");
    }

    #[test]
    fn hls_output() {
        let args = hls_args(
            &Source::test_pattern(),
            &Encoding::default(),
            &HlsSettings::default(),
        );
        let s = strings(&args);
        assert_eq!(
            &s[..5],
            ["-hide_banner", "-nostats", "-loglevel", "warning", "-y"]
        );
        assert_eq!(after(&s, "-f"), "lavfi");
        assert_eq!(
            &s[s.len() - 11..],
            [
                "-f",
                "hls",
                "-hls_time",
                "2",
                "-hls_list_size",
                "6",
                "-hls_flags",
                "independent_segments+program_date_time+delete_segments",
                "-hls_segment_filename",
                "seg%05d.ts",
                "index.m3u8",
            ]
        );
        let kept = hls_args(
            &Source::test_pattern(),
            &Encoding::default(),
            &HlsSettings::default().keep_segments().with_segment_secs(4),
        );
        let s = strings(&kept);
        assert_eq!(
            after(&s, "-hls_flags"),
            "independent_segments+program_date_time"
        );
        assert_eq!(after(&s, "-hls_time"), "4");
    }

    #[test]
    fn record_and_concat() {
        let args = record_args(&Source::test_pattern(), &Encoding::default(), &part_name(3));
        let s = strings(&args);
        assert_eq!(&s[s.len() - 3..], ["-f", "matroska", "part-0003.mkv"]);
        assert_eq!(
            strings(&concat_args())[5..],
            [
                "-f",
                "concat",
                "-i",
                "parts.txt",
                "-c",
                "copy",
                "-movflags",
                "+faststart",
                "recording.mp4"
            ]
        );
        assert_eq!(
            concat_list(&["part-0001.mkv", "it's.mkv"]),
            "file 'part-0001.mkv'\nfile 'it'\\''s.mkv'\n"
        );
    }

    /// §Presenting the Stream Key: appended to the RTMP application path.
    #[test]
    fn rtmp_key_is_the_stream_name() {
        for uri in [
            "rtmp://live.example.com/app",
            "rtmp://live.example.com/app/",
        ] {
            let t = PushTarget::new(uri, Some("sk_live_abc123")).unwrap();
            let args = push_args(&Source::test_pattern(), &Encoding::default(), &t);
            let s = strings(&args);
            assert_eq!(&s[5..9], ["-progress", "progress", "-stats_period", "2"]);
            assert_eq!(
                &s[s.len() - 3..],
                ["-f", "flv", "rtmp://live.example.com/app/sk_live_abc123"]
            );
        }
        let t = PushTarget::new("RTMPS://live.example.com:443/app", Some("k")).unwrap();
        assert_eq!(t.url, "rtmps://live.example.com:443/app/k");
        assert_eq!(t.scheme(), "rtmp");
        let none = PushTarget::new("rtmp://live.example.com/app/key-in-uri", None).unwrap();
        assert_eq!(none.url, "rtmp://live.example.com/app/key-in-uri");
    }

    /// §Presenting the Stream Key: the SRT stream ID, unless the URI has
    /// one.
    #[test]
    fn srt_key_is_the_stream_id() {
        let t = PushTarget::new("srt://ingest.example.com:9000", Some("live/abc")).unwrap();
        let args = push_args(&Source::test_pattern(), &Encoding::default(), &t);
        let s = strings(&args);
        assert_eq!(
            &s[s.len() - 3..],
            [
                "-f",
                "mpegts",
                "srt://ingest.example.com:9000?streamid=live%2Fabc"
            ]
        );
        let given =
            PushTarget::new("srt://ingest.example.com:9000?streamid=mine", Some("k")).unwrap();
        assert_eq!(given.url, "srt://ingest.example.com:9000?streamid=mine");
    }

    /// §Presenting the Stream Key: the WHIP bearer token (RFC 9725).
    #[test]
    fn whip_key_is_the_bearer_token() {
        let t = PushTarget::new("https://whip.example.com/ingest/1", Some("tok")).unwrap();
        let args = push_args(&Source::test_pattern(), &Encoding::default(), &t);
        let s = strings(&args);
        assert_eq!(
            &s[s.len() - 5..],
            [
                "-f",
                "whip",
                "-authorization",
                "tok",
                "https://whip.example.com/ingest/1"
            ]
        );
    }

    #[test]
    fn refused_destinations() {
        for (uri, scheme) in [
            ("file:///etc/passwd", "file"),
            ("http://whip.example.com/x", "http"),
            ("udp://10.0.0.1:1234", "udp"),
        ] {
            match PushTarget::new(uri, Some("k")) {
                Err(FfmpegError::UnsupportedScheme(s)) => assert_eq!(s, scheme),
                other => panic!("{uri}: {other:?}"),
            }
        }
        assert!(matches!(
            PushTarget::new("not a uri", None),
            Err(FfmpegError::InvalidStreamUri)
        ));
        assert!(matches!(
            PushTarget::new("rtmp:nohost", None),
            Err(FfmpegError::InvalidStreamUri)
        ));
    }

    /// Security testing pattern 1 (workspace AGENTS.md): the stream key
    /// never reaches `Debug`.
    #[test]
    fn push_target_debug_redacts_the_key() {
        for uri in [
            "rtmp://live.example.com/app",
            "srt://live.example.com:9000",
            "https://live.example.com/whip",
        ] {
            let t = PushTarget::new(uri, Some("canary-key-5d2e")).unwrap();
            let shown = format!("{t:?}");
            assert!(!shown.contains("canary-key-5d2e"), "{shown}");
            assert!(shown.contains("live.example.com"), "{shown}");
        }
    }
}
