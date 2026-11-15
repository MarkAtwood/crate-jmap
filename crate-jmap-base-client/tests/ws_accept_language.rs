//! `Accept-Language` on the WebSocket opening handshake
//! (draft-atwood-jmap-updates-00 §4.3, jmap-chat-spec 2b85363: "A server
//! SHOULD treat the Accept-Language header of the WebSocket opening
//! handshake as the Accept-Language of every Request on that
//! connection").
//!
//! Oracle: the header the server receives, compared with the value the
//! test passes in; RFC 9110 §12.5.4 field name `Accept-Language`.

use jmap_base_client::{ClientError, WsConnectOptions, connect_ws_with_options};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

/// Accept one connection and return the handshake's Accept-Language.
async fn handshake_accept_language(options: WsConnectOptions) -> Option<String> {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut seen = None;
        // The callback signature, and its large ErrorResponse type, is
        // tungstenite's.
        #[allow(clippy::result_large_err)]
        let callback = |req: &Request, resp: Response| {
            seen = req
                .headers()
                .get("accept-language")
                .map(|v| v.to_str().unwrap().to_owned());
            Ok(resp)
        };
        let _ws = tokio_tungstenite::accept_hdr_async(stream, callback)
            .await
            .unwrap();
        seen
    });
    let session = connect_ws_with_options(
        &format!("ws://{addr}/"),
        None,
        &options.allow_plaintext(true),
    )
    .await
    .unwrap();
    drop(session);
    server.await.unwrap()
}

#[tokio::test]
async fn sends_accept_language_when_set() {
    let seen = handshake_accept_language(
        WsConnectOptions::new().accept_language(Some("fr-CH, fr;q=0.9".to_owned())),
    )
    .await;
    assert_eq!(seen.as_deref(), Some("fr-CH, fr;q=0.9"));
}

#[tokio::test]
async fn sends_no_accept_language_by_default() {
    assert_eq!(
        handshake_accept_language(WsConnectOptions::new()).await,
        None
    );
}

/// A value that is not a valid header value is rejected before connecting.
#[tokio::test]
async fn rejects_invalid_header_value() {
    let err = connect_ws_with_options(
        "wss://127.0.0.1:1/",
        None,
        &WsConnectOptions::new().accept_language(Some("fr\nX: y".to_owned())),
    )
    .await
    .map(|_| ())
    .unwrap_err();
    assert!(matches!(err, ClientError::InvalidArgument(_)), "{err:?}");
}
