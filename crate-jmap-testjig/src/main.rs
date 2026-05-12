//! Entry point for the `jmap-testjig` binary.
//!
//! See the crate-level docs in `lib.rs` for the NOT-FOR-PRODUCTION
//! disclaimer. The full CLI (port / token flags, structured startup
//! banner) lands in slice `bd:JMAP-cf7p.8`; this slice
//! (`bd:JMAP-cf7p.2`) wires the foundation `POST /jmap` and
//! `GET /.well-known/jmap` routes and binds on a hardcoded
//! `127.0.0.1:8080`.
//!
//! Graceful shutdown on Ctrl-C is handled via `tokio::signal::ctrl_c`.

#![forbid(unsafe_code)]

use std::net::SocketAddr;

use jmap_testjig::http::{router, AppState};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Hardcoded bind address; slice bd:JMAP-cf7p.8 will surface this
    // as a CLI flag. `127.0.0.1` is intentional — the testjig is
    // localhost-only and never advertises remote access.
    let addr: SocketAddr = "127.0.0.1:8080"
        .parse()
        .expect("hardcoded socket address must be valid");

    let app = router(AppState::new());

    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!(
        "jmap-testjig: listening on http://{addr} \
         (NOT FOR PRODUCTION — in-memory only, single-user, no auth integration)"
    );
    eprintln!("  GET  /.well-known/jmap   — RFC 8620 §2 Session resource");
    eprintln!("  POST /jmap               — RFC 8620 §3 API endpoint");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Wait for Ctrl-C (SIGINT) and resolve the future so axum's
/// `with_graceful_shutdown` returns. Errors from `ctrl_c` mean the
/// signal handler could not be installed; in that case we sleep
/// forever and let the operator kill the process by other means
/// rather than crashing the testjig at startup.
async fn shutdown_signal() {
    if let Err(err) = tokio::signal::ctrl_c().await {
        eprintln!("jmap-testjig: ctrl_c signal handler unavailable: {err}; running until killed");
        std::future::pending::<()>().await;
    }
}
