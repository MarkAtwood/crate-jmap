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
//! # Scaffold status
//!
//! This scaffold slice (`bd:JMAP-cf7p.1`) sets up the crate directory and
//! dependency graph only. The actual routes, handler wiring, SSE / WS
//! endpoints, auth middleware, and `spawn_in_process` API land in
//! subsequent slices (`bd:JMAP-cf7p.2` through `.7`).

#![forbid(unsafe_code)]
