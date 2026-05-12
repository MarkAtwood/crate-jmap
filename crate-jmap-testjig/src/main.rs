//! Entry point for the `jmap-testjig` binary.
//!
//! See the crate-level docs in `lib.rs` for the NOT-FOR-PRODUCTION
//! disclaimer. This binary parses two optional CLI flags
//! (`--port <port>` and `--token <token>`) and binds an axum HTTP
//! server with the testjig router on `127.0.0.1:<port>`. The default
//! port is 8080 and the default token is
//! [`DEFAULT_BEARER_TOKEN`]. Graceful shutdown on Ctrl-C is handled
//! via `tokio::signal::ctrl_c`.
//!
//! There are no other flags, no config files, and no environment
//! variables. Anything more complicated belongs in a downstream
//! consumer (kith, stoa, or your own JMAP server).

#![forbid(unsafe_code)]

use std::net::SocketAddr;

use jmap_testjig::auth::DEFAULT_BEARER_TOKEN;
use jmap_testjig::http::{router, AppState};

/// Default TCP port the testjig binds when `--port` is not supplied.
const DEFAULT_PORT: u16 = 8080;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let cli = match Cli::parse(std::env::args().skip(1)) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("jmap-testjig: {err}");
            eprintln!();
            print_usage();
            std::process::exit(2);
        }
    };

    // `127.0.0.1` is intentional — the testjig is localhost-only
    // and never advertises remote access. Operators who need
    // off-host reachability should put a TLS-terminating reverse
    // proxy in front of the testjig (or, better, build a real
    // server on the workspace's library kit).
    let addr: SocketAddr = SocketAddr::from(([127, 0, 0, 1], cli.port));

    let app = router(AppState::with_token(cli.token.clone()));

    let listener = tokio::net::TcpListener::bind(addr).await?;
    // `local_addr()` reflects the actual bound port (when `--port 0`
    // requests an OS-assigned port, this is how the operator finds
    // the actual port).
    let bound = listener.local_addr()?;
    print_banner(bound, &cli.token);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Print the startup banner. Loudly identifies the process as the
/// testjig so operators do not mistake it for a production server.
fn print_banner(addr: SocketAddr, token: &str) {
    eprintln!("===========================================================");
    eprintln!(" jmap-testjig — NOT FOR PRODUCTION");
    eprintln!("   In-memory only · Single-user · No auth integration");
    eprintln!("   No persistence · No TLS · No quotas · No CORS");
    eprintln!("===========================================================");
    eprintln!("Listening on http://{addr}");
    eprintln!("  GET  /.well-known/jmap   — RFC 8620 §2 Session");
    eprintln!("  POST /jmap               — RFC 8620 §3 API");
    eprintln!("  GET  /events             — RFC 8620 §7.3 SSE");
    eprintln!("  GET  /ws                 — RFC 8887 WebSocket");
    eprintln!("  Authorization: Bearer {token}");
    eprintln!();
    eprintln!("Ctrl-C to stop.");
}

/// Print invocation help. Shown on `--help`, `-h`, and on flag-parse
/// failure (e.g. missing argument to `--port`).
fn print_usage() {
    eprintln!("USAGE: jmap-testjig [--port <port>] [--token <token>]");
    eprintln!();
    eprintln!("FLAGS:");
    eprintln!("  --port <port>     TCP port on 127.0.0.1 (default: {DEFAULT_PORT}; use 0 for an OS-assigned port)");
    eprintln!(
        "  --token <token>   Bearer token clients must supply (default: {DEFAULT_BEARER_TOKEN})"
    );
    eprintln!("  -h, --help        Print this help and exit");
    eprintln!();
    eprintln!("NOT FOR PRODUCTION. See crate README for the kit-vs-jig posture.");
}

/// Parsed CLI flags. Populated by [`Cli::parse`] from `env::args`.
#[derive(Debug)]
struct Cli {
    port: u16,
    token: String,
}

impl Cli {
    /// Parse flags from an iterator of arguments (skipping argv[0]).
    ///
    /// Returns `Err(message)` on missing argument, unknown flag, or
    /// invalid port number. The error string is printed to stderr
    /// by `main` followed by [`print_usage`].
    ///
    /// Recognised flags:
    /// - `--port <port>`  (default: [`DEFAULT_PORT`])
    /// - `--token <token>` (default: [`DEFAULT_BEARER_TOKEN`])
    /// - `--help` / `-h`  (prints usage; main exits with status 0)
    fn parse<I>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = String>,
    {
        let mut port: u16 = DEFAULT_PORT;
        let mut token: String = DEFAULT_BEARER_TOKEN.to_owned();
        let mut iter = args.into_iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--port" => {
                    let v = iter
                        .next()
                        .ok_or_else(|| "--port requires an argument".to_owned())?;
                    port = v.parse().map_err(|_| format!("--port: not a u16: {v}"))?;
                }
                "--token" => {
                    token = iter
                        .next()
                        .ok_or_else(|| "--token requires an argument".to_owned())?;
                }
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                other => return Err(format!("unknown flag: {other}")),
            }
        }
        Ok(Cli { port, token })
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle: omitting both flags yields the documented defaults.
    #[test]
    fn parse_no_args_uses_defaults() {
        let cli = Cli::parse(std::iter::empty()).expect("no-arg parse");
        assert_eq!(cli.port, DEFAULT_PORT);
        assert_eq!(cli.token, DEFAULT_BEARER_TOKEN);
    }

    /// Oracle: `--port 9090` populates the port field; `--token foo`
    /// populates the token field.
    #[test]
    fn parse_overrides_both_flags() {
        let cli = Cli::parse(
            ["--port", "9090", "--token", "custom"]
                .into_iter()
                .map(str::to_owned),
        )
        .expect("override parse");
        assert_eq!(cli.port, 9090);
        assert_eq!(cli.token, "custom");
    }

    /// Oracle: order between the two flags is irrelevant. The
    /// `--token` flag can come before `--port`.
    #[test]
    fn parse_flag_order_is_free() {
        let cli = Cli::parse(
            ["--token", "abc", "--port", "0"]
                .into_iter()
                .map(str::to_owned),
        )
        .expect("reverse-order parse");
        assert_eq!(cli.port, 0);
        assert_eq!(cli.token, "abc");
    }

    /// Oracle: `--port` requires an argument; bare `--port` is an
    /// error.
    #[test]
    fn parse_missing_port_arg_errors() {
        let err = Cli::parse(["--port"].into_iter().map(str::to_owned))
            .expect_err("missing arg must error");
        assert!(err.contains("--port"));
    }

    /// Oracle: a non-numeric port value produces a useful error.
    #[test]
    fn parse_bad_port_value_errors() {
        let err = Cli::parse(["--port", "abc"].into_iter().map(str::to_owned))
            .expect_err("bad port must error");
        assert!(err.contains("--port"));
        assert!(err.contains("abc"));
    }

    /// Oracle: unknown flags are reported by name.
    #[test]
    fn parse_unknown_flag_errors() {
        let err = Cli::parse(["--bogus"].into_iter().map(str::to_owned))
            .expect_err("unknown flag must error");
        assert!(err.contains("--bogus"));
    }
}
