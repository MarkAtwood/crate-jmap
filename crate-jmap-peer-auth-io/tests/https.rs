//! `HttpsFetcher` against a local TLS stub server, for
//! draft-atwood-jmap-chat-federation-00 §Key Publication: the receiving
//! server fetches "over HTTPS, validating the server certificate against
//! the peer domain", and "MAY follow redirects, provided each one is to
//! an `https` URI whose certificate it validates the same way; the fetch
//! is subject to {{ssrf}}."
//!
//! The fixtures are a test CA and a `localhost` certificate it signed,
//! made with OpenSSL (`tests/fixtures/`). They are test-only keys.

use std::net::IpAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use jmap_peer_auth_io::{Fetch, FetchError, HttpsFetcher};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

const CA: &[u8] = include_bytes!("fixtures/ca.pem");
const CERT: &[u8] = include_bytes!("fixtures/localhost.pem");
const KEY: &[u8] = include_bytes!("fixtures/localhost.key");

/// Reads one request head and returns its request target.
async fn read_target(stream: &mut (impl AsyncReadExt + Unpin)) -> String {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).await.unwrap_or(0) == 0 {
            break;
        }
        head.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&head);
    head.split(' ').nth(1).unwrap_or_default().to_owned()
}

/// A TLS server on 127.0.0.1 presenting the `localhost` certificate.
/// `route` maps a request target to a full HTTP/1.1 response. Returns the
/// port and a count of accepted TCP connections.
async fn tls_server(
    route: impl Fn(&str) -> String + Send + Sync + 'static,
) -> (u16, Arc<AtomicUsize>) {
    let cert = CertificateDer::from_pem_slice(CERT).unwrap();
    let key = PrivateKeyDer::from_pem_slice(KEY).unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let route = Arc::new(route);
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                return;
            };
            counter.fetch_add(1, Ordering::SeqCst);
            let acceptor = acceptor.clone();
            let route = Arc::clone(&route);
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(tcp).await else {
                    return;
                };
                let target = read_target(&mut tls).await;
                let _ = tls.write_all(route(&target).as_bytes()).await;
                let _ = tls.shutdown().await;
            });
        }
    });
    (port, hits)
}

/// A plain-HTTP listener on `ip` that counts the TCP connections it
/// accepts.
async fn http_server(ip: &str) -> (u16, Arc<AtomicUsize>) {
    let listener = TcpListener::bind((ip, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    tokio::spawn(async move {
        while let Ok((mut tcp, _)) = listener.accept().await {
            counter.fetch_add(1, Ordering::SeqCst);
            read_target(&mut tcp).await;
            let _ = tcp
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}")
                .await;
        }
    });
    (port, hits)
}

fn ok(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncache-control: max-age=300\r\n\
         content-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn redirect(location: &str) -> String {
    format!(
        "HTTP/1.1 302 Found\r\nlocation: {location}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
    )
}

/// A client that trusts only the test CA.
fn builder() -> jmap_peer_auth_io::reqwest::ClientBuilder {
    let ca = jmap_peer_auth_io::reqwest::Certificate::from_pem(CA).unwrap();
    jmap_peer_auth_io::reqwest::Client::builder()
        .tls_built_in_root_certs(false)
        .add_root_certificate(ca)
        .no_proxy()
}

fn fetcher() -> HttpsFetcher {
    HttpsFetcher::with_policy(builder(), |_| true).unwrap()
}

const SESSION: &str = r#"{"ownerUserId":"alice@localhost","peerSigningKeys":[]}"#;

#[tokio::test]
async fn fetches_over_https_with_a_valid_certificate() {
    let (port, _) = tls_server(|_| ok(SESSION)).await;
    let response = fetcher()
        .get(&format!("https://localhost:{port}/.well-known/jmap"))
        .await
        .unwrap();
    assert_eq!(response.body, SESSION.as_bytes());
    assert_eq!(response.headers["cache-control"], "max-age=300");
}

/// "MAY follow redirects, provided each one is to an `https` URI".
#[tokio::test]
async fn follows_a_redirect_to_https() {
    let (port, _) = tls_server(|target| match target {
        "/.well-known/jmap" => redirect("/jmap/session"),
        _ => ok(SESSION),
    })
    .await;
    let response = fetcher()
        .get(&format!("https://localhost:{port}/.well-known/jmap"))
        .await
        .unwrap();
    assert_eq!(response.body, SESSION.as_bytes());
}

/// A redirect to an `http` URI is not followed: the plain-HTTP target
/// never receives a connection.
#[tokio::test]
async fn refuses_a_redirect_to_http() {
    let (http_port, hits) = http_server("127.0.0.1").await;
    let (port, _) =
        tls_server(move |_| redirect(&format!("http://localhost:{http_port}/.well-known/jmap")))
            .await;
    let result = fetcher()
        .get(&format!("https://localhost:{port}/.well-known/jmap"))
        .await;
    assert!(
        matches!(result, Err(FetchError::Transport(_))),
        "{result:?}"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

/// "whose certificate it validates the same way": the redirect target is
/// reached, but its `localhost` certificate has no `127.0.0.1` identity,
/// so the hop fails.
#[tokio::test]
async fn refuses_a_redirect_whose_certificate_does_not_match() {
    let (target_port, target_hits) = tls_server(|_| ok(SESSION)).await;
    let (port, _) =
        tls_server(move |_| redirect(&format!("https://127.0.0.1:{target_port}/"))).await;
    let result = fetcher()
        .get(&format!("https://localhost:{port}/.well-known/jmap"))
        .await;
    assert!(
        matches!(result, Err(FetchError::Transport(_))),
        "{result:?}"
    );
    assert_eq!(target_hits.load(Ordering::SeqCst), 1);
}

/// "validating the server certificate against the peer domain".
#[tokio::test]
async fn rejects_a_certificate_for_another_host() {
    let (port, _) = tls_server(|_| ok(SESSION)).await;
    let result = fetcher()
        .get(&format!("https://127.0.0.1:{port}/.well-known/jmap"))
        .await;
    assert!(
        matches!(result, Err(FetchError::Transport(_))),
        "{result:?}"
    );
}

/// §Blob Fetch and SSRF: servers "MUST NOT connect to link-local,
/// loopback, or private-network addresses in response to a peer-supplied
/// URL". A policy refusing loopback stops the fetch of a name that
/// resolves to loopback before any connection.
#[tokio::test]
async fn the_address_policy_refuses_resolved_addresses() {
    let (port, hits) = tls_server(|_| ok(SESSION)).await;
    let fetcher = HttpsFetcher::with_policy(builder(), |ip: IpAddr| !ip.is_loopback()).unwrap();
    let result = fetcher
        .get(&format!("https://localhost:{port}/.well-known/jmap"))
        .await;
    assert!(
        matches!(result, Err(FetchError::Transport(_))),
        "{result:?}"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

/// The policy also covers an IP-literal redirect target, which is
/// connected to without name resolution: a listener on the refused
/// address never receives a connection.
#[tokio::test]
async fn the_address_policy_refuses_ip_literal_redirects() {
    let (refused_port, refused_hits) = http_server("127.0.0.2").await;
    let (port, _) =
        tls_server(move |_| redirect(&format!("https://127.0.0.2:{refused_port}/"))).await;
    let refused: IpAddr = "127.0.0.2".parse().unwrap();
    let fetcher = HttpsFetcher::with_policy(builder(), move |ip| ip != refused).unwrap();
    let result = fetcher
        .get(&format!("https://localhost:{port}/.well-known/jmap"))
        .await;
    assert!(
        matches!(result, Err(FetchError::Transport(_))),
        "{result:?}"
    );
    assert_eq!(refused_hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_non_2xx_status_is_an_error() {
    let (port, _) = tls_server(|_| {
        "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_owned()
    })
    .await;
    let result = fetcher()
        .get(&format!("https://localhost:{port}/.well-known/jmap"))
        .await;
    assert!(matches!(result, Err(FetchError::Status(404))), "{result:?}");
}
