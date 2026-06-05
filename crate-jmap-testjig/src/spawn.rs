//! In-process spawn helper for per-client-crate integration tests.
//!
//! Per-client integration tests (in `jmap-mail-client`,
//! `jmap-chat-client`, etc.) need a running JMAP server to exercise
//! the full client + transport + dispatcher + backend stack. Standing
//! up a separate process for each test is expensive and fragile —
//! port collisions, slow startup, subprocess management. This module
//! spawns the testjig inside the test's own tokio runtime instead.
//!
//! # NOT FOR PRODUCTION
//!
//! `spawn_in_process` carries the same NOT-FOR-PRODUCTION posture as
//! the rest of this crate. It serves the in-memory MemoryBackends,
//! a hardcoded bearer token, and a single hardcoded account. The
//! helper exists for tests; do not call it from production code.
//!
//! # Lifetime
//!
//! The returned [`TestjigHandle`] owns the tokio task running the
//! server. Dropping the handle aborts the task, which closes the
//! listening socket and ends every in-flight connection. Tests that
//! want graceful shutdown (e.g. to drain in-flight requests) should
//! call [`TestjigHandle::shutdown`] explicitly before drop.
//!
//! # Example
//!
//! ```rust,ignore
//! use jmap_testjig::{spawn_in_process, TestjigConfig};
//!
//! #[tokio::test]
//! async fn my_client_test() {
//!     let jig = spawn_in_process(TestjigConfig::default())
//!         .await
//!         .expect("spawn jig");
//!     let url = format!("http://{}/.well-known/jmap", jig.addr);
//!     let session = reqwest::Client::new()
//!         .get(&url)
//!         .bearer_auth(&jig.token)
//!         .send()
//!         .await
//!         .expect("session");
//!     assert!(session.status().is_success());
//!     // Drop drops the JoinHandle → aborts the task → closes the socket.
//! }
//! ```

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use crate::auth::DEFAULT_BEARER_TOKEN;
use crate::http::{router, AppState};

/// Configuration for [`spawn_in_process`].
///
/// The default is fine for the vast majority of integration tests:
/// random ephemeral port, default bearer token (`test-token`),
/// loopback bind address.
#[derive(Clone, Debug)]
pub struct TestjigConfig {
    /// TCP port to bind. `0` (the default) requests a random free
    /// ephemeral port from the OS, which is the right choice for
    /// parallel test execution — fixed ports collide.
    pub port: u16,

    /// Bearer token clients must supply. Defaults to
    /// [`DEFAULT_BEARER_TOKEN`] so integration tests that use the
    /// same default-token convention as the binary work without
    /// configuration.
    pub token: String,

    /// Loopback address to bind. Defaults to `127.0.0.1`. Tests
    /// that need IPv6 reachability can set `[::1]` here. There is no
    /// supported value other than a loopback address — the testjig
    /// is not safe to expose off-host.
    pub ip: IpAddr,
}

impl Default for TestjigConfig {
    fn default() -> Self {
        Self {
            port: 0,
            token: DEFAULT_BEARER_TOKEN.to_owned(),
            ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
        }
    }
}

/// Handle to a running in-process testjig.
///
/// Owns the tokio task running the server. Dropping aborts the task
/// (closing the listening socket and any in-flight connections).
/// Calling [`Self::shutdown`] explicitly aborts the task and awaits
/// its completion, which is the cleaner pattern when tests want to
/// guarantee the port is free before continuing.
///
/// `Send`/`Sync` are satisfied because every field is `Send + Sync`:
/// [`SocketAddr`], [`String`], and [`JoinHandle<()>`] all implement
/// both. Tests can move the handle across tokio task boundaries.
#[must_use = "TestjigHandle aborts the server when dropped; ignoring it tears down the jig immediately"]
pub struct TestjigHandle {
    /// The actually-bound socket address. When [`TestjigConfig::port`]
    /// is `0`, this exposes the random port the OS assigned so tests
    /// can build URLs against the right port without poking sockets.
    pub addr: SocketAddr,

    /// The bearer token clients must include in
    /// `Authorization: Bearer <token>` headers (or as `?token=...`).
    /// Echoed from [`TestjigConfig::token`].
    pub token: String,

    /// The tokio task running the axum server. Held inside an
    /// [`Option`] so [`Self::shutdown`] can take it, abort it, and
    /// await it without consuming `self` (the Drop impl needs to be
    /// able to fire on the residual `None`).
    task: Option<JoinHandle<()>>,
}

impl TestjigHandle {
    /// Abort the server task and await its completion.
    ///
    /// Use this when a test needs the port to be free before
    /// continuing (e.g. spawning a second jig on a fixed port).
    /// For the common case where the test ends with the handle going
    /// out of scope, simply letting Drop fire is sufficient.
    pub async fn shutdown(mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
            // Awaiting a JoinHandle that was just aborted returns
            // Err(JoinError::cancelled). We don't surface that —
            // the task ending is the only thing we care about.
            let _ = task.await;
        }
    }
}

impl Drop for TestjigHandle {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
            // We cannot `await` inside Drop. Abort is best-effort;
            // the task may still be in the middle of a syscall when
            // the handle drops. Tests that need synchronous teardown
            // should call `shutdown()` explicitly.
        }
    }
}

impl std::fmt::Debug for TestjigHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestjigHandle")
            .field("addr", &self.addr)
            // Token is short and not credential-grade for this
            // crate's purposes (a hardcoded test token), but
            // redacting in Debug keeps it from accidentally appearing
            // in test failure output if someone passes a real
            // production-shaped token.
            .field("token", &"[REDACTED]")
            .field("task_alive", &self.task.is_some())
            .finish()
    }
}

/// Spawn the testjig on a tokio task and return a handle.
///
/// Binds a [`TcpListener`] on [`TestjigConfig::ip`]:[`TestjigConfig::port`],
/// builds the full router (POST /jmap + GET /.well-known/jmap with
/// all 7 reference MemoryBackends mounted and bearer-auth applied),
/// and spawns `axum::serve(...)` on `tokio::spawn`. The returned
/// handle's [`TestjigHandle::addr`] reflects the actual bound
/// address (with the OS-assigned port when port=0 was requested).
///
/// # Errors
///
/// Returns [`std::io::Error`] if the TCP bind fails. The most common
/// failure mode is `EADDRINUSE` when port != 0 and the port is
/// already taken; that case is the test author's responsibility.
///
/// # Cancellation safety
///
/// Cancelling this future before it returns leaves no resources
/// behind: the only async step is `TcpListener::bind`, after which
/// the spawn is synchronous and the server task is owned by the
/// returned handle.
pub async fn spawn_in_process(config: TestjigConfig) -> std::io::Result<TestjigHandle> {
    let listener = TcpListener::bind(SocketAddr::new(config.ip, config.port)).await?;
    let addr = listener.local_addr()?;

    let app = router(AppState::with_token(config.token.clone()));
    let token = config.token;

    let task = tokio::spawn(async move {
        // The serve future runs forever or until the listener errors.
        // Errors here are unrecoverable for the testjig — log to
        // stderr and let the task end. The handle's owner will see
        // task termination via `is_finished` or via failed requests.
        if let Err(err) = axum::serve(listener, app).await {
            eprintln!("jmap-testjig: serve loop ended: {err}");
        }
    });

    Ok(TestjigHandle {
        addr,
        token,
        task: Some(task),
    })
}

// Compile-time check that the handle is Send + Sync as the bead
// requires. A regression on this (e.g. by adding a non-Sync field
// to TestjigHandle) would break per-client integration tests that
// move the handle across task boundaries.
#[allow(dead_code)]
fn _assert_handle_send_sync() {
    fn check<T: Send + Sync>() {}
    check::<TestjigHandle>();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle: spawn binds on the configured port (when port=0, the
    /// OS picks a random port and we get it back via local_addr).
    #[tokio::test]
    async fn spawn_returns_real_bound_port() {
        let jig = spawn_in_process(TestjigConfig::default())
            .await
            .expect("spawn");
        assert_ne!(
            jig.addr.port(),
            0,
            "port=0 in config must resolve to a real OS-assigned port in the handle"
        );
        assert_eq!(jig.addr.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
    }

    /// Oracle: the handle exposes the same token the config supplied,
    /// so tests can build Authorization headers without hardcoding.
    #[tokio::test]
    async fn spawn_handle_echoes_configured_token() {
        let config = TestjigConfig {
            token: "custom-test-token-abc".to_owned(),
            ..TestjigConfig::default()
        };
        let jig = spawn_in_process(config).await.expect("spawn");
        assert_eq!(jig.token, "custom-test-token-abc");
    }

    /// Oracle: bd:JMAP-cf7p.7 acceptance — Drop aborts the task.
    /// We verify by holding a JoinHandle outside the handle and
    /// asserting it completes after the handle is dropped.
    #[tokio::test]
    async fn drop_aborts_server_task() {
        let jig = spawn_in_process(TestjigConfig::default())
            .await
            .expect("spawn");
        let addr = jig.addr;
        // Confirm the server is up before tearing down.
        let probe = tokio::net::TcpStream::connect(addr).await;
        assert!(probe.is_ok(), "server must accept connections before drop");
        drop(probe);
        drop(jig);
        // Give the runtime a moment to process the abort. The
        // listener should be closed by now; connect should fail.
        for _ in 0..50 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            if tokio::net::TcpStream::connect(addr).await.is_err() {
                return;
            }
        }
        panic!("server did not stop accepting connections within 500 ms of handle drop");
    }

    /// Oracle: bd:JMAP-cf7p.7 acceptance — shutdown() aborts the task
    /// and awaits its completion, so the port is free synchronously
    /// when the call returns.
    #[tokio::test]
    async fn shutdown_awaits_task_completion() {
        let jig = spawn_in_process(TestjigConfig::default())
            .await
            .expect("spawn");
        let addr = jig.addr;
        jig.shutdown().await;
        // shutdown() must have awaited the task, so any further
        // connect attempt must fail immediately. We retry briefly
        // to account for kernel-side socket close latency, but
        // unlike drop_aborts_server_task the bound on "soon" is
        // much tighter — we expect this in well under 100 ms.
        for _ in 0..10 {
            if tokio::net::TcpStream::connect(addr).await.is_err() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("server still accepting connections after explicit shutdown");
    }
}
