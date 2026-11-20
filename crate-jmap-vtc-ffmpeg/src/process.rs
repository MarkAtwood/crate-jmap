//! Running ffmpeg: the [`ProcessRunner`] seam and its tokio
//! implementation.
//!
//! [`FfmpegMedia`](crate::FfmpegMedia) starts and supervises its
//! processes only through these two traits, so tests substitute a fake
//! runner and need no ffmpeg. [`TokioRunner`] is the real one: it spawns
//! the program directly with [`tokio::process::Command`] (no shell), its
//! stdin a pipe for the graceful stop, stdout discarded.

use std::ffi::OsString;
use std::fmt;
use std::future::Future;
use std::io;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, Command};

/// How a process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessExit {
    code: Option<i32>,
}

impl ProcessExit {
    /// An exit with status `code`, or `None` for a process ended by a
    /// signal.
    pub fn new(code: Option<i32>) -> Self {
        Self { code }
    }

    /// The exit status, `None` when a signal ended the process.
    pub fn code(&self) -> Option<i32> {
        self.code
    }

    /// `true` for exit status 0.
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

impl fmt::Display for ProcessExit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.code {
            Some(code) => write!(f, "exit status {code}"),
            None => f.write_str("a signal"),
        }
    }
}

/// Starts processes.
pub trait ProcessRunner: Send + Sync {
    /// The handle of a started process.
    type Process: MediaProcess;

    /// Start `program` with `args`, one argument each and no shell, in
    /// `current_dir`.
    ///
    /// # Errors
    ///
    /// The I/O error of the spawn, for example a `program` that is not
    /// installed.
    fn spawn(
        &self,
        program: &Path,
        args: &[OsString],
        current_dir: &Path,
    ) -> io::Result<Self::Process>;
}

/// A started process.
pub trait MediaProcess: Send {
    /// How the process ended, or `None` while it runs. Does not block.
    ///
    /// # Errors
    ///
    /// The I/O error of the status check.
    fn try_wait(&mut self) -> io::Result<Option<ProcessExit>>;

    /// Wait for the process to end.
    ///
    /// # Errors
    ///
    /// The I/O error of the wait.
    fn wait(&mut self) -> impl Future<Output = io::Result<ProcessExit>> + Send;

    /// Ask the process to finish cleanly: ffmpeg reads `q` on its stdin,
    /// stops reading input and writes its trailers (the HLS
    /// `EXT-X-ENDLIST`, the Matroska index). Returns without waiting. A
    /// process that has already ended is not an error.
    ///
    /// # Errors
    ///
    /// The I/O error of the request.
    fn request_stop(&mut self) -> impl Future<Output = io::Result<()>> + Send;

    /// End the process at once.
    ///
    /// # Errors
    ///
    /// The I/O error of the kill.
    fn kill(&mut self) -> impl Future<Output = io::Result<()>> + Send;
}

/// [`MediaProcess::request_stop`], then wait up to `grace` for the
/// process to end, then [`MediaProcess::kill`] it.
pub(crate) async fn finish<P: MediaProcess>(
    process: &mut P,
    grace: Duration,
) -> io::Result<ProcessExit> {
    if let Some(exit) = process.try_wait()? {
        return Ok(exit);
    }
    if process.request_stop().await.is_ok()
        && let Ok(exit) = tokio::time::timeout(grace, process.wait()).await
    {
        return exit;
    }
    process.kill().await?;
    process.wait().await
}

/// Spawns real processes with tokio.
#[derive(Debug, Clone, Default)]
pub struct TokioRunner {
    inherit_stderr: bool,
}

impl TokioRunner {
    /// A runner that discards the processes' stderr.
    pub fn new() -> Self {
        Self::default()
    }

    /// This runner passing the processes' stderr through to this
    /// process's, for debugging. ffmpeg's warnings and errors then show,
    /// and nothing else: the arguments ask for `-loglevel warning` and no
    /// stats line.
    pub fn inherit_stderr(self) -> Self {
        Self {
            inherit_stderr: true,
        }
    }
}

impl ProcessRunner for TokioRunner {
    type Process = TokioProcess;

    fn spawn(
        &self,
        program: &Path,
        args: &[OsString],
        current_dir: &Path,
    ) -> io::Result<TokioProcess> {
        let stderr = if self.inherit_stderr {
            Stdio::inherit()
        } else {
            Stdio::null()
        };
        let mut child = Command::new(program)
            .args(args)
            .current_dir(current_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(stderr)
            .kill_on_drop(true)
            .spawn()?;
        let stdin = child.stdin.take();
        Ok(TokioProcess { child, stdin })
    }
}

/// A process [`TokioRunner`] started. Dropping it kills the process.
#[derive(Debug)]
pub struct TokioProcess {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl MediaProcess for TokioProcess {
    fn try_wait(&mut self) -> io::Result<Option<ProcessExit>> {
        Ok(self
            .child
            .try_wait()?
            .map(|status| ProcessExit::new(status.code())))
    }

    async fn wait(&mut self) -> io::Result<ProcessExit> {
        let status = self.child.wait().await?;
        Ok(ProcessExit::new(status.code()))
    }

    async fn request_stop(&mut self) -> io::Result<()> {
        // Closing stdin after the `q` does not stop ffmpeg by itself; the
        // `q` does.
        let Some(mut stdin) = self.stdin.take() else {
            return Ok(());
        };
        match stdin.write_all(b"q").await {
            Ok(()) => stdin.flush().await.or_else(ignore_broken_pipe),
            Err(e) => ignore_broken_pipe(e),
        }
    }

    async fn kill(&mut self) -> io::Result<()> {
        match self.child.kill().await {
            // Already ended and reaped.
            Err(e) if e.kind() == io::ErrorKind::InvalidInput => Ok(()),
            other => other,
        }
    }
}

/// A process that has already ended has closed its stdin.
fn ignore_broken_pipe(e: io::Error) -> io::Result<()> {
    if e.kind() == io::ErrorKind::BrokenPipe {
        Ok(())
    } else {
        Err(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_display() {
        assert_eq!(ProcessExit::new(Some(1)).to_string(), "exit status 1");
        assert_eq!(ProcessExit::new(None).to_string(), "a signal");
        assert!(ProcessExit::new(Some(0)).success());
        assert!(!ProcessExit::new(None).success());
    }

    /// A program that is not installed is a spawn error, not a panic.
    #[tokio::test]
    async fn missing_program_is_an_error() {
        let err = TokioRunner::new()
            .spawn(
                Path::new("/nonexistent/jmap-vtc-ffmpeg-no-such-program"),
                &[],
                &std::env::temp_dir(),
            )
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    /// The graceful stop closes stdin after the `q`; `cat` (standing in
    /// for a process that ends on request) then exits 0 within the grace.
    #[cfg(unix)]
    #[tokio::test]
    async fn finish_stops_gracefully() {
        let mut p = TokioRunner::new()
            .spawn(Path::new("cat"), &[], &std::env::temp_dir())
            .unwrap();
        assert_eq!(p.try_wait().unwrap(), None);
        let exit = finish(&mut p, Duration::from_secs(10)).await.unwrap();
        assert!(exit.success(), "{exit}");
        // Stopping again is fine.
        p.request_stop().await.unwrap();
    }

    /// A process that ignores the request is killed after the grace.
    #[cfg(unix)]
    #[tokio::test]
    async fn finish_kills_after_the_grace() {
        let mut p = TokioRunner::new()
            .spawn(Path::new("sleep"), &["30".into()], &std::env::temp_dir())
            .unwrap();
        let exit = finish(&mut p, Duration::from_millis(100)).await.unwrap();
        assert_eq!(exit.code(), None, "{exit}");
        p.kill().await.unwrap();
    }
}
