// Streaming size-cap integration tests (bd:JMAP-6r7c.1).
//
// These tests exercise the per-chunk streaming cap in `read_capped_body`.
// They use a raw-TCP server (not wiremock) because wiremock builds responses
// through hyper, which always sets Content-Length. To prove the streaming
// guard fires when Content-Length is *absent* (chunked Transfer-Encoding
// against a hostile or compromised server, RFC 7230 §3.3.1), we have to
// hand-build the HTTP/1.1 response bytes ourselves.
//
// Distinguishing the fix from a regression:
//
//   The hostile server sends one HTTP/1.1 chunked frame just large enough
//   to push the accumulated body 1 byte past the configured cap, then
//   stops sending — no terminating `0\r\n\r\n` frame, no FIN. A post-fix
//   client returns `ClientError::ResponseTooLarge` as soon as the
//   cap-crossing chunk arrives. A pre-fix client (which calls
//   `resp.bytes().await` to drain the whole body before the cap check
//   fires) blocks forever waiting for end-of-stream and the test times
//   out.
//
// The 2-second test timeout would not be enough to expose a future bug
// that simply prefers to buffer all of a *bounded* oversized body; tests
// in `client_tests.rs` cover that fast-path Content-Length rejection.
// The point of these tests is the streaming-cap branch specifically.

use std::time::Duration;

use jmap_base_client::auth::NoneAuth;
use jmap_base_client::client::{ClientConfig, JmapClient};
use jmap_base_client::error::ClientError;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::timeout;

/// Spawn an HTTP/1.1 raw-socket server that, for every incoming connection,
/// reads the request headers, sends a `Transfer-Encoding: chunked` response
/// header block, then writes a single chunk of `chunk_size` bytes — and then
/// holds the socket open indefinitely without sending the terminating
/// zero-length chunk.
///
/// Returns the bound socket address. The server task runs forever; it is
/// implicitly torn down when the tokio runtime exits at test end.
async fn spawn_chunked_no_eof_server(chunk_size: usize) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");

    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                // Drain request headers. We do not parse them — the test only
                // needs to wait until the client has finished sending the
                // request line and headers (terminated by `\r\n\r\n`).
                let mut buf = [0u8; 4096];
                let mut seen: Vec<u8> = Vec::with_capacity(4096);
                loop {
                    let n = match sock.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => n,
                    };
                    seen.extend_from_slice(&buf[..n]);
                    if seen.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                    if seen.len() > 16 * 1024 {
                        return;
                    }
                }

                // Send response headers — Transfer-Encoding: chunked, no
                // Content-Length. This is the exact shape RFC 7230 §3.3.1
                // requires for a streaming chunked response.
                let head = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
                if sock.write_all(head).await.is_err() {
                    return;
                }

                // Send a single chunk of `chunk_size` bytes in proper
                // HTTP/1.1 chunked framing: `<hex-size>\r\n<bytes>\r\n`.
                let size_line = format!("{chunk_size:x}\r\n");
                if sock.write_all(size_line.as_bytes()).await.is_err() {
                    return;
                }
                let payload = vec![b'x'; chunk_size];
                if sock.write_all(&payload).await.is_err() {
                    return;
                }
                if sock.write_all(b"\r\n").await.is_err() {
                    return;
                }

                // Do NOT send the terminating zero-length chunk. Hold the
                // connection open until the runtime tears the task down at
                // test end. A pre-fix client that buffers the entire body
                // via resp.bytes().await will block here indefinitely; the
                // post-fix streaming-cap path returns ResponseTooLarge as
                // soon as the cap-crossing chunk lands.
                let _ = sock.flush().await;
                std::future::pending::<()>().await;
            });
        }
    });

    addr
}

fn build_client_with_session_cap(base_url: &str, cap: u64) -> JmapClient {
    let mut config = ClientConfig::default();
    config.max_session_body = cap;
    config.request_timeout = Duration::from_secs(30);
    JmapClient::new(
        jmap_base_client::auth::DefaultTransport,
        NoneAuth,
        base_url,
        config,
    )
    .expect("client construction must succeed")
}

fn build_client_with_call_cap(base_url: &str, cap: u64) -> JmapClient {
    let mut config = ClientConfig::default();
    config.max_call_body = cap;
    config.request_timeout = Duration::from_secs(30);
    JmapClient::new(
        jmap_base_client::auth::DefaultTransport,
        NoneAuth,
        base_url,
        config,
    )
    .expect("client construction must succeed")
}

fn build_client_with_upload_cap(base_url: &str, cap: u64) -> JmapClient {
    let mut config = ClientConfig::default();
    config.max_upload_response_body = cap;
    config.request_timeout = Duration::from_secs(30);
    JmapClient::new(
        jmap_base_client::auth::DefaultTransport,
        NoneAuth,
        base_url,
        config,
    )
    .expect("client construction must succeed")
}

fn build_client_with_download_cap(base_url: &str, cap: u64) -> JmapClient {
    let mut config = ClientConfig::default();
    config.max_download_body = cap;
    config.request_timeout = Duration::from_secs(30);
    JmapClient::new(
        jmap_base_client::auth::DefaultTransport,
        NoneAuth,
        base_url,
        config,
    )
    .expect("client construction must succeed")
}

fn minimal_request() -> jmap_types::JmapRequest {
    jmap_types::JmapRequest::new(
        vec!["urn:ietf:params:jmap:core".to_owned()],
        vec![(
            "Mailbox/get".to_owned(),
            serde_json::json!({"accountId": "A13824", "ids": null}),
            "r1".to_owned(),
        )],
        None,
    )
}

/// Oracle: bd:JMAP-6r7c.1 — fetch_session must enforce the body cap
/// per-chunk, not after buffering the whole body.
///
/// A hostile server sending chunked Transfer-Encoding with no Content-Length
/// and never finishing the stream must NOT be able to force unbounded
/// allocation by the client. The post-fix code returns `ResponseTooLarge`
/// as soon as the streaming cap is crossed; the pre-fix code waits for
/// end-of-stream and would hang here.
#[tokio::test]
async fn fetch_session_streams_with_cap_against_chunked_hostile_server() {
    // 128-byte cap; server sends a 256-byte chunk in one frame — comfortably
    // past the cap so the cap-crossing decision happens on the first chunk.
    let cap: u64 = 128;
    let addr = spawn_chunked_no_eof_server(256).await;
    let base = format!("http://{addr}");
    let client = build_client_with_session_cap(&base, cap);

    let result = timeout(Duration::from_secs(2), client.fetch_session()).await;

    let err = result
        .expect(
            "fetch_session must return within 2s after the streaming cap fires; a hang \
             indicates the pre-fix .bytes().await path is still in use",
        )
        .expect_err("oversized chunked response must surface ResponseTooLarge");

    assert!(
        matches!(err, ClientError::ResponseTooLarge { .. }),
        "expected ResponseTooLarge, got {err:?}"
    );
}

/// Oracle: bd:JMAP-6r7c.1 — call() must enforce the body cap per-chunk.
#[tokio::test]
async fn call_streams_with_cap_against_chunked_hostile_server() {
    let cap: u64 = 128;
    let addr = spawn_chunked_no_eof_server(256).await;
    let base = format!("http://{addr}");
    let client = build_client_with_call_cap(&base, cap);
    let req = minimal_request();
    let api_url = format!("{base}/api");

    let result = timeout(Duration::from_secs(2), client.call(&api_url, &req)).await;

    let err = result
        .expect(
            "call() must return within 2s after the streaming cap fires; a hang indicates \
             the pre-fix .bytes().await path is still in use",
        )
        .expect_err("oversized chunked response must surface ResponseTooLarge");

    assert!(
        matches!(err, ClientError::ResponseTooLarge { .. }),
        "expected ResponseTooLarge, got {err:?}"
    );
}

/// Oracle: bd:JMAP-6r7c.1 — upload_blob() must enforce the body cap per-chunk.
#[tokio::test]
async fn upload_blob_streams_with_cap_against_chunked_hostile_server() {
    let cap: u64 = 128;
    let addr = spawn_chunked_no_eof_server(256).await;
    let base = format!("http://{addr}");
    let client = build_client_with_upload_cap(&base, cap);
    let upload_url = format!("{base}/upload/{{accountId}}");

    let result = timeout(
        Duration::from_secs(2),
        client.upload_blob(
            &upload_url,
            "account1",
            bytes::Bytes::from_static(b"payload"),
            "application/octet-stream",
        ),
    )
    .await;

    let err = result
        .expect(
            "upload_blob() must return within 2s after the streaming cap fires; a hang \
             indicates the pre-fix .bytes().await path is still in use",
        )
        .expect_err("oversized chunked response must surface ResponseTooLarge");

    assert!(
        matches!(err, ClientError::ResponseTooLarge { .. }),
        "expected ResponseTooLarge, got {err:?}"
    );
}

/// Oracle: bd:JMAP-6r7c.1 — download_blob already used the streaming cap;
/// this test pins that behavior so a future refactor cannot regress it.
#[tokio::test]
async fn download_blob_streams_with_cap_against_chunked_hostile_server() {
    let cap: u64 = 128;
    let addr = spawn_chunked_no_eof_server(256).await;
    let base = format!("http://{addr}");
    let client = build_client_with_download_cap(&base, cap);
    let download_url = format!("{base}/download/{{accountId}}/{{blobId}}/{{name}}");

    let result = timeout(
        Duration::from_secs(2),
        client.download_blob(jmap_base_client::DownloadBlobParams {
            download_url_template: &download_url,
            account_id: "account1",
            blob_id: "blob-abc",
            name: "file.bin",
            accept_type: None,
            expected_sha256: None,
        }),
    )
    .await;

    let err = result
        .expect(
            "download_blob() must return within 2s after the streaming cap fires; a hang \
             indicates a regression to a .bytes().await path",
        )
        .expect_err("oversized chunked response must surface ResponseTooLarge");

    assert!(
        matches!(err, ClientError::ResponseTooLarge { .. }),
        "expected ResponseTooLarge, got {err:?}"
    );
}
