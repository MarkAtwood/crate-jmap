//! `AssetFetcher` against a local TLS stub server, for
//! draft-atwood-jmap-scene-00 §assetUri Is Untrusted
//! (jmap-chat-spec/draft-atwood-jmap-scene-00.md L2395-2397):
//! "Clients MUST validate that `assetUri` uses a permitted scheme
//! (`https:`) before fetching. Clients MUST NOT follow redirects to
//! non-HTTPS endpoints." and "Clients SHOULD verify asset integrity using
//! the `sha256` field on SceneAsset when available."
//!
//! The fixtures are the workspace's test CA and `localhost` certificate
//! (copied from jmap-peer-auth-io, made with OpenSSL). They are test-only
//! keys. The digests are FIPS 180-4 example vectors.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use jmap_cid_types::Sha256;
use jmap_scene_client::{AssetFetchError, AssetFetcher};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

const CA: &[u8] = include_bytes!("fixtures/ca.pem");
const CERT: &[u8] = include_bytes!("fixtures/localhost.pem");
const KEY: &[u8] = include_bytes!("fixtures/localhost.key");

/// FIPS 180-4 SHA-256 of "abc".
const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
/// FIPS 180-4 SHA-256 of the empty message.
const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

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
/// `route` maps a request target to a full HTTP/1.1 response.
async fn tls_server(route: impl Fn(&str) -> String + Send + Sync + 'static) -> u16 {
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
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
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
    port
}

/// A plain-HTTP listener on 127.0.0.1 that counts the TCP connections it
/// accepts.
async fn http_server() -> (u16, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    tokio::spawn(async move {
        while let Ok((mut tcp, _)) = listener.accept().await {
            counter.fetch_add(1, Ordering::SeqCst);
            read_target(&mut tcp).await;
            let _ = tcp
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 3\r\nconnection: close\r\n\r\nabc")
                .await;
        }
    });
    (port, hits)
}

fn ok(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: model/gltf-binary\r\ncontent-length: {}\r\n\
         connection: close\r\n\r\n{body}",
        body.len()
    )
}

fn redirect(location: &str) -> String {
    format!(
        "HTTP/1.1 302 Found\r\nlocation: {location}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
    )
}

/// A fetcher that trusts only the test CA.
fn fetcher() -> AssetFetcher {
    let ca = jmap_scene_client::reqwest::Certificate::from_pem(CA).unwrap();
    let builder = jmap_scene_client::reqwest::Client::builder()
        .tls_built_in_root_certs(false)
        .add_root_certificate(ca)
        .no_proxy();
    AssetFetcher::with_builder(builder).unwrap()
}

fn digest(hex: &str) -> Sha256 {
    hex.parse().unwrap()
}

/// "Clients MUST validate that `assetUri` uses a permitted scheme
/// (`https:`) before fetching": an `http:` assetUri is refused and the
/// server never receives a connection.
#[tokio::test]
async fn http_asset_uri_refused_before_fetching() {
    let (port, hits) = http_server().await;
    let result = fetcher()
        .fetch(&format!("http://localhost:{port}/a.glb"), None, 1024)
        .await;
    assert!(
        matches!(result, Err(AssetFetchError::NotHttps)),
        "{result:?}"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn non_uri_and_other_schemes_refused() {
    let result = fetcher().fetch("not a uri", None, 1024).await;
    assert!(
        matches!(result, Err(AssetFetchError::InvalidUri(_))),
        "{result:?}"
    );
    for uri in ["ftp://localhost/a.glb", "file:///etc/passwd", "data:,abc"] {
        let result = fetcher().fetch(uri, None, 1024).await;
        assert!(
            matches!(result, Err(AssetFetchError::NotHttps)),
            "{uri}: {result:?}"
        );
    }
}

/// "Clients MUST NOT follow redirects to non-HTTPS endpoints": the
/// plain-HTTP target never receives a connection.
#[tokio::test]
async fn redirect_to_http_not_followed() {
    let (http_port, hits) = http_server().await;
    let port = tls_server(move |_| redirect(&format!("http://localhost:{http_port}/a.glb"))).await;
    let result = fetcher()
        .fetch(&format!("https://localhost:{port}/a.glb"), None, 1024)
        .await;
    assert!(
        matches!(result, Err(AssetFetchError::Transport(_))),
        "{result:?}"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn redirect_to_https_followed() {
    let port = tls_server(|target| match target {
        "/old.glb" => redirect("/a.glb"),
        _ => ok("abc"),
    })
    .await;
    let body = fetcher()
        .fetch(
            &format!("https://localhost:{port}/old.glb"),
            Some(&digest(ABC)),
            1024,
        )
        .await
        .unwrap();
    assert_eq!(body, b"abc");
}

/// "Clients SHOULD verify asset integrity using the `sha256` field": a
/// body whose FIPS 180-4 digest is not the expected one fails.
#[tokio::test]
async fn sha256_mismatch_detected() {
    let port = tls_server(|_| ok("abc")).await;
    let result = fetcher()
        .fetch(
            &format!("https://localhost:{port}/a.glb"),
            Some(&digest(EMPTY)),
            1024,
        )
        .await;
    match result {
        Err(AssetFetchError::DigestMismatch { expected, actual }) => {
            assert_eq!(expected, EMPTY);
            assert_eq!(actual, ABC);
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn body_over_the_limit_refused() {
    let port = tls_server(|_| ok("abc")).await;
    let result = fetcher()
        .fetch(&format!("https://localhost:{port}/a.glb"), None, 2)
        .await;
    assert!(
        matches!(result, Err(AssetFetchError::TooLarge(2))),
        "{result:?}"
    );
}

#[tokio::test]
async fn non_2xx_status_is_an_error() {
    let port = tls_server(|_| {
        "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_owned()
    })
    .await;
    let result = fetcher()
        .fetch(&format!("https://localhost:{port}/a.glb"), None, 1024)
        .await;
    assert!(
        matches!(result, Err(AssetFetchError::Status(404))),
        "{result:?}"
    );
}
