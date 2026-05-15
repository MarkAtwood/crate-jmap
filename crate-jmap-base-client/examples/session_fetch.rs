//! Fetch and inspect a JMAP Session object (RFC 8620 §2).
//!
//! By default the example runs offline: it starts an in-process `wiremock`
//! server that serves a hand-written §2 Session fixture, then points the
//! client at it. Set `JMAP_TEST_URL` to target a real JMAP endpoint instead;
//! optionally set `JMAP_TEST_TOKEN` to include a bearer token.
//!
//! Either way the flow is `new_plain` → `fetch_session` → print the username,
//! the URL fields, the capabilities the server advertises, and each account.
//!
//! Run with:
//!
//! ```sh
//! cargo run --example session_fetch -p jmap-base-client
//! ```
//!
//! Or against a real server:
//!
//! ```sh
//! JMAP_TEST_URL=https://jmap.example.com \
//! JMAP_TEST_TOKEN=my-bearer-token \
//!     cargo run --example session_fetch -p jmap-base-client
//! ```

use jmap_base_client::{BearerAuth, ClientConfig, JmapClient, NoneAuth, Session};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// RFC 8620 §2 Session example, hand-written from the spec.
const SESSION_FIXTURE: &str = r#"{
  "capabilities": {
    "urn:ietf:params:jmap:core": {
      "maxSizeUpload": 50000000,
      "maxConcurrentUpload": 8,
      "maxSizeRequest": 10000000,
      "maxConcurrentRequest": 8,
      "maxCallsInRequest": 32,
      "maxObjectsInGet": 256,
      "maxObjectsInSet": 128,
      "collationAlgorithms": ["i;ascii-numeric", "i;ascii-casemap", "i;unicode-casemap"]
    },
    "urn:ietf:params:jmap:mail": {}
  },
  "accounts": {
    "A13824": {
      "name": "john@example.com",
      "isPersonal": true,
      "isReadOnly": false,
      "accountCapabilities": {
        "urn:ietf:params:jmap:mail": { "maxMailboxesPerEmail": null, "maxMailboxDepth": 10 }
      }
    }
  },
  "primaryAccounts": {
    "urn:ietf:params:jmap:core": "A13824",
    "urn:ietf:params:jmap:mail": "A13824"
  },
  "username": "john@example.com",
  "apiUrl": "https://jmap.example.com/api/",
  "downloadUrl": "https://jmap.example.com/download/{accountId}/{blobId}/{name}?accept={type}",
  "uploadUrl": "https://jmap.example.com/upload/{accountId}/",
  "eventSourceUrl": "https://jmap.example.com/eventsource/?types={types}&closeafter={closeafter}&ping={ping}",
  "state": "75128aab4b1b"
}"#;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // _server holds the wiremock guard alive for offline runs; it is unused
    // when JMAP_TEST_URL is set (Option::None).
    let (base_url, _server) = match std::env::var("JMAP_TEST_URL") {
        Ok(url) => (url, None),
        Err(_) => {
            let server = start_mock_server().await;
            let url = server.uri();
            (url, Some(server))
        }
    };

    let session = match std::env::var("JMAP_TEST_TOKEN") {
        Ok(token) => {
            let auth = BearerAuth::new(&token)?;
            JmapClient::new_plain(auth, &base_url, ClientConfig::default())?
                .fetch_session()
                .await?
        }
        Err(_) => {
            JmapClient::new_plain(NoneAuth, &base_url, ClientConfig::default())?
                .fetch_session()
                .await?
        }
    };

    print_session(&session);
    Ok(())
}

/// Print the typed Session fields. Maps are sorted so output is deterministic.
///
/// Two fields are deliberately rendered as redaction placeholders instead
/// of their literal values (bd:JMAP-6r7c.9, bd:JMAP-6r7c.60):
///
/// - `session.username` is the authenticated user's identifier — typically
///   a full email address, which is PII under GDPR/CCPA. The `Session`
///   type's manual `Debug` impl already redacts it; this example must not
///   route around that redaction via `Display`.
/// - `session.state` is the RFC 8620 §2 session-state token. It is not an
///   auth credential, but it uniquely identifies the client's session and
///   is the same shape of leak as logging a session cookie.
///
/// The other URL fields are deployment metadata, not credentials — print
/// them verbatim for diagnostics. Production code that wants the typed
/// values should access them via accessor methods or `{:?}`, both of
/// which respect the redaction.
fn print_session(session: &Session) {
    // session.username is PII (typically an email); session.state is a
    // session-identifying opaque token (cookie-grade). Both are
    // intentionally rendered as placeholders here — see fn doc.
    println!("username:        [REDACTED]");
    println!("api_url:         {}", session.api_url);
    println!("upload_url:      {}", session.upload_url);
    println!("download_url:    {}", session.download_url);
    println!("event_source:    {}", session.event_source_url);
    println!("state:           [opaque]");

    let mut caps: Vec<&String> = session.capabilities.keys().collect();
    caps.sort();
    println!("\ncapabilities ({}):", caps.len());
    for cap in caps {
        println!("  - {cap}");
    }

    let mut account_ids: Vec<&String> = session.accounts.keys().collect();
    account_ids.sort();
    println!("\naccounts ({}):", account_ids.len());
    for id in account_ids {
        let info = &session.accounts[id];
        println!(
            "  - {id}  (personal={}, read_only={})",
            info.is_personal, info.is_read_only,
        );
    }

    let mut prims: Vec<(&String, &String)> = session.primary_accounts.iter().collect();
    prims.sort();
    println!("\nprimary accounts ({}):", prims.len());
    for (cap, acct) in prims {
        println!("  - {cap} => {acct}");
    }
}

/// Spin up a wiremock server that answers `GET /.well-known/jmap` with the
/// hand-written §2 fixture above.
async fn start_mock_server() -> MockServer {
    let body: serde_json::Value =
        serde_json::from_str(SESSION_FIXTURE).expect("SESSION_FIXTURE must be valid JSON");
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/.well-known/jmap"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    server
}
