//! In-process JMAP test jig — **NOT a JMAP server consumers should run**.
//!
//! This crate is internal scaffolding for the workspace's own integration
//! testing and demonstration. It is `publish = false` and will never appear
//! on crates.io.
//!
//! # NOT FOR PRODUCTION
//!
//! - **In-memory only.** Every backend stores state in `HashMap`s behind a
//!   `std::sync::Mutex`. Restarting the process loses all data.
//! - **Single-user, single-account.** One hardcoded account-id under one
//!   hardcoded principal. No multi-tenancy, no per-request identity.
//! - **No auth integration.** A hardcoded bearer token gates every endpoint
//!   (default `test-token`). No OAuth, no JWT, no mTLS, no upstream IdP.
//! - **No persistence, no quotas, no backups, no rate-limiting.**
//! - **No TLS termination.** The testjig binds plaintext HTTP on
//!   `127.0.0.1`. Operators terminate TLS in front of it if remote access
//!   is needed (and there is no good reason for remote access here).
//!
//! # Audiences
//!
//! 1. **Workspace integration tests** — per-crate integration tests can
//!    spawn the jig in-process via a `spawn_in_process` helper (filed as
//!    `bd:JMAP-cf7p.7`) and exercise method handlers across all 8
//!    extensions without standing up a database.
//! 2. **Smoke testing** — `cargo run -p jmap-testjig` starts a working
//!    JMAP server on `127.0.0.1:8080`; hit it with curl or any JMAP
//!    client to verify the workspace's extension surface end-to-end.
//! 3. **Contributor onboarding** — clone the repo, run the jig, see a
//!    working JMAP server.
//!
//! Production deployments must instead build their own dispatcher and HTTP
//! layer on top of the workspace's library kit (`jmap-server` foundation,
//! the 8 `jmap-*-server` extension crates, plus the consumer's own
//! transport, auth, persistence, and multi-tenancy). See the workspace
//! `AGENTS.md` "What this workspace builds" section.
//!
//! # Slice status
//!
//! - `bd:JMAP-cf7p.1` (closed): crate scaffold + dependency wiring.
//! - `bd:JMAP-cf7p.2` (closed): axum [`http::router`] with
//!   `POST /jmap` (RFC 8620 §3 dispatch) and
//!   `GET /.well-known/jmap` (RFC 8620 §2 Session). A built-in
//!   `Core/echo` handler (RFC 8620 §4) is registered so the
//!   dispatcher demonstrates end-to-end request/response flow.
//! - `bd:JMAP-cf7p.3` (closed): 8 reference MemoryBackends wired.
//! - `bd:JMAP-cf7p.4` (closed): SSE endpoint [`sse::get_events`]
//!   exposing `GET /events` (RFC 8620 §7.3) that pushes
//!   [RFC 8620 §7.1] `StateChange` events derived from a tight
//!   polling loop across all 8 backends.
//! - `bd:JMAP-cf7p.5` (this slice): WebSocket endpoint
//!   [`ws::get_ws`] exposing `GET /ws` (RFC 8887) that frames
//!   `Request`/`Response`/`RequestError`/`StateChange` envelopes
//!   over a `jmap` subprotocol connection.
//! - `bd:JMAP-cf7p.6` (closed): bearer-token middleware.
//! - `bd:JMAP-cf7p.7` (closed): [`spawn_in_process`] for tests.
//! - Remaining slices (`.8`): README + rustdoc warnings.
//!
//! [RFC 8620 §7.1]: https://www.rfc-editor.org/rfc/rfc8620.html#section-7.1

#![forbid(unsafe_code)]

pub mod auth;
pub mod http;
pub mod session;
pub mod spawn;
pub mod sse;
pub mod ws;

pub use spawn::{spawn_in_process, TestjigConfig, TestjigHandle};
