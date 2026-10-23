//! `CustomCaTransport` trust scope over a real TLS handshake
//! (bd:JMAP-xh47, follow-up to bd:JMAP-6r7c.57).
//!
//! `CustomCaTransport` documents that the configured private CA is the
//! *only* trust anchor. These tests run a real rustls server and check
//! that a client pinned to CA B accepts B's leaf and rejects a leaf from
//! an unrelated CA A with `UnknownIssuer`.
//!
//! Oracle: the fixtures under `tests/fixtures/tls/trust-scope/` were made
//! with the OpenSSL 3.0 CLI, not with this crate. Each `ca-<n>.der` is a
//! self-signed P-256 CA. Each `leaf-<n>.der` is a server certificate for
//! `IP:127.0.0.1` signed by `ca-<n>`, and `leaf-<n>.key.der` is its PKCS#8
//! key. All are valid for 36500 days. `openssl verify -CAfile ca-a.pem
//! leaf-a.pem` and `openssl verify -CAfile ca-b.pem leaf-b.pem` both return
//! OK. `openssl verify -CAfile ca-b.pem leaf-a.pem` fails with "unable to
//! get local issuer certificate". The session body is the hand-written RFC
//! 8620 §2 fixture in `tests/fixtures/jmap/session.json`.

use std::sync::Arc;

use jmap_base_client::{ClientConfig, ClientError, CustomCaTransport, JmapClient, NoneAuth};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use tokio_rustls::rustls::{self, CertificateError, ServerConfig};

const CA_A: &[u8] = include_bytes!("fixtures/tls/trust-scope/ca-a.der");
const CA_B: &[u8] = include_bytes!("fixtures/tls/trust-scope/ca-b.der");
const LEAF_A: &[u8] = include_bytes!("fixtures/tls/trust-scope/leaf-a.der");
const LEAF_A_KEY: &[u8] = include_bytes!("fixtures/tls/trust-scope/leaf-a.key.der");
const LEAF_B: &[u8] = include_bytes!("fixtures/tls/trust-scope/leaf-b.der");
const LEAF_B_KEY: &[u8] = include_bytes!("fixtures/tls/trust-scope/leaf-b.key.der");
const SESSION_JSON: &str = include_str!("fixtures/jmap/session.json");

/// Starts a one-connection HTTPS server on 127.0.0.1 that presents `leaf`.
/// After a successful handshake it answers one request with the session
/// fixture. Returns the base URL.
async fn serve_once(leaf: &'static [u8], key: &'static [u8]) -> String {
    let config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .expect("ring supports the default protocol versions")
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(leaf)],
                PrivatePkcs8KeyDer::from(key).into(),
            )
            .expect("fixture leaf certificate and key must match");
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("local_addr").port();

    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.expect("accept");
        // A rejected handshake ends here: the client aborts the connection.
        let Ok(mut tls) = acceptor.accept(tcp).await else {
            return;
        };
        let mut request = Vec::new();
        let mut buf = [0u8; 1024];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = tls.read(&mut buf).await.expect("read request");
            assert!(n > 0, "client closed before sending a full request");
            request.extend_from_slice(&buf[..n]);
        }
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{SESSION_JSON}",
            SESSION_JSON.len()
        );
        tls.write_all(response.as_bytes())
            .await
            .expect("write response");
        tls.shutdown().await.expect("shutdown");
    });

    format!("https://127.0.0.1:{port}")
}

fn client_trusting(ca: &[u8], base_url: &str) -> JmapClient {
    JmapClient::new(
        CustomCaTransport::new(ca.to_vec()),
        NoneAuth,
        base_url,
        ClientConfig::default(),
    )
    .expect("client construction must succeed for a valid DER CA")
}

/// Finds the rustls error in `err`'s source chain. reqwest wraps it in
/// nested `std::io::Error`s, whose `source()` skips the wrapped error
/// itself, so `io::Error` links are descended with `get_ref` instead.
fn rustls_error_in_chain<'a>(
    err: &'a (dyn std::error::Error + 'static),
) -> Option<&'a rustls::Error> {
    let mut cur = Some(err);
    while let Some(e) = cur {
        if let Some(r) = e.downcast_ref::<rustls::Error>() {
            return Some(r);
        }
        cur = match e.downcast_ref::<std::io::Error>() {
            Some(io) => io
                .get_ref()
                .map(|inner| inner as &(dyn std::error::Error + 'static)),
            None => e.source(),
        };
    }
    None
}

/// The pinned private CA is trusted: a leaf it signed completes the
/// handshake and the RFC 8620 §2 session parses.
#[tokio::test]
async fn pinned_ca_accepts_its_own_leaf() {
    let url = serve_once(LEAF_B, LEAF_B_KEY).await;
    let session = client_trusting(CA_B, &url)
        .fetch_session()
        .await
        .expect("leaf-b is signed by the pinned ca-b");
    assert_eq!(session.username, "john@example.com");
}

/// Control for the rejection test below: server A's fixture is a valid
/// chain, so pinning ca-a is enough to reach it.
#[tokio::test]
async fn server_a_is_reachable_when_its_ca_is_pinned() {
    let url = serve_once(LEAF_A, LEAF_A_KEY).await;
    client_trusting(CA_A, &url)
        .fetch_session()
        .await
        .expect("leaf-a is signed by the pinned ca-a");
}

/// A client pinned to ca-b rejects a leaf from any other CA. The failure
/// is `ClientError::Http` and its source chain holds rustls
/// `InvalidCertificate(UnknownIssuer)`.
#[tokio::test]
async fn pinned_ca_rejects_leaf_from_other_ca() {
    let url = serve_once(LEAF_A, LEAF_A_KEY).await;
    let err = client_trusting(CA_B, &url)
        .fetch_session()
        .await
        .expect_err("leaf-a is not signed by the pinned ca-b");
    let ClientError::Http(http) = &err else {
        panic!("expected ClientError::Http, got {err:?}");
    };
    assert_eq!(
        rustls_error_in_chain(http),
        Some(&rustls::Error::InvalidCertificate(
            CertificateError::UnknownIssuer
        )),
        "source chain of {http:?}"
    );
}
