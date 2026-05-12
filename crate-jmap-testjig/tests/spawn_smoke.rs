//! Integration test for the `spawn_in_process` public API
//! (bd:JMAP-cf7p.7).
//!
//! This test exercises the testjig the way a downstream client crate
//! would: spawn the jig on a random port, point an HTTP client at
//! it, walk the foundation endpoints, drop the handle, confirm the
//! server stops cleanly. It is the "kitchen sink" smoke test for
//! the testjig as a whole — if any earlier slice regressed
//! (router wiring, auth, MemoryBackend mounting, spawn helper),
//! this test will catch it.

use jmap_testjig::{spawn_in_process, TestjigConfig};
use reqwest::StatusCode;
use serde_json::{json, Value};

/// Oracle: bd:JMAP-cf7p.7 acceptance — the spawned jig serves
/// `GET /.well-known/jmap` with all 9 advertised capability URIs
/// (one core + 8 workspace extensions). Confirms the router is
/// wired, auth accepts the configured token, and the Session JSON
/// includes the right URIs.
#[tokio::test]
async fn spawn_in_process_serves_session_with_all_capabilities() {
    let jig = spawn_in_process(TestjigConfig::default())
        .await
        .expect("spawn must succeed");

    let client = reqwest::Client::builder()
        .build()
        .expect("reqwest client build must succeed");

    let url = format!("http://{}/.well-known/jmap", jig.addr);
    let resp = client
        .get(&url)
        .bearer_auth(&jig.token)
        .send()
        .await
        .expect("session request must succeed");
    assert_eq!(resp.status(), StatusCode::OK);

    let session: Value = resp.json().await.expect("session body must be JSON");

    let caps = session["capabilities"]
        .as_object()
        .expect("capabilities must be a JSON object per RFC 8620 §2");

    // Core + 8 extensions = 9 advertised URIs.
    let expected = [
        "urn:ietf:params:jmap:core",
        "urn:ietf:params:jmap:mail",
        "urn:ietf:params:jmap:chat",
        "urn:ietf:params:jmap:calendars",
        "urn:ietf:params:jmap:tasks",
        "urn:ietf:params:jmap:contacts",
        "urn:ietf:params:jmap:filenode",
        "urn:ietf:params:jmap:sharing",
        "urn:ietf:params:jmap:metadata",
    ];
    for uri in expected {
        assert!(
            caps.contains_key(uri),
            "Session.capabilities missing {uri}: {caps:?}"
        );
    }
}

/// Oracle: bd:JMAP-cf7p.7 acceptance — the spawned jig dispatches
/// `Core/echo` (RFC 8620 §4) and the echo is byte-for-byte
/// equivalent to the request args.
#[tokio::test]
async fn spawn_in_process_dispatches_core_echo() {
    let jig = spawn_in_process(TestjigConfig::default())
        .await
        .expect("spawn must succeed");

    let client = reqwest::Client::new();
    let url = format!("http://{}/jmap", jig.addr);
    let req = json!({
        "using": ["urn:ietf:params:jmap:core"],
        "methodCalls": [
            ["Core/echo", {"hello": "world", "n": 42}, "c1"]
        ]
    });
    let resp = client
        .post(&url)
        .bearer_auth(&jig.token)
        .json(&req)
        .send()
        .await
        .expect("echo request must succeed");
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.expect("echo response must be JSON");
    let inv = &body["methodResponses"][0];
    assert_eq!(inv[0], "Core/echo");
    assert_eq!(inv[1], json!({"hello": "world", "n": 42}));
    assert_eq!(inv[2], "c1");
}

/// Oracle: bd:JMAP-cf7p.7 acceptance — auth gates the spawned jig
/// the same way it gates the binary. A request without the bearer
/// token returns 401.
#[tokio::test]
async fn spawn_in_process_enforces_bearer_auth() {
    let jig = spawn_in_process(TestjigConfig::default())
        .await
        .expect("spawn must succeed");

    let client = reqwest::Client::new();
    let url = format!("http://{}/.well-known/jmap", jig.addr);
    // No bearer_auth call — request is anonymous.
    let resp = client
        .get(&url)
        .send()
        .await
        .expect("HTTP request must complete");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// Oracle: bd:JMAP-cf7p.7 acceptance — dropping the handle stops
/// the server. After drop, subsequent connect attempts must fail.
#[tokio::test]
async fn spawn_in_process_drop_stops_server() {
    let jig = spawn_in_process(TestjigConfig::default())
        .await
        .expect("spawn must succeed");
    let addr = jig.addr;

    // Sanity-check: the server is live before drop.
    let client = reqwest::Client::new();
    let pre_url = format!("http://{addr}/.well-known/jmap");
    let pre_resp = client
        .get(&pre_url)
        .bearer_auth(&jig.token)
        .send()
        .await
        .expect("pre-drop request must succeed");
    assert_eq!(pre_resp.status(), StatusCode::OK);

    drop(jig);

    // After the handle is dropped, the task aborts and the listener
    // closes. The next connection attempt must fail within a short
    // window. We retry briefly because tokio's abort + kernel-side
    // socket close are asynchronous.
    let mut stopped = false;
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        // Use a fresh client every iteration so any pooled
        // connection from the pre-drop request is not reused.
        let probe = reqwest::Client::builder()
            .pool_idle_timeout(std::time::Duration::from_millis(0))
            .build()
            .unwrap();
        let probe_url = format!("http://{addr}/.well-known/jmap");
        if probe.get(&probe_url).send().await.is_err() {
            stopped = true;
            break;
        }
    }
    assert!(
        stopped,
        "server must stop accepting connections within 500 ms of handle drop"
    );
}

/// Oracle: bd:JMAP-cf7p.7 acceptance — a custom-configured token
/// authenticates correctly end-to-end. This is the path real
/// integration tests will take (each test picks a unique token to
/// avoid cross-test interference if they share a runtime).
#[tokio::test]
async fn spawn_in_process_honors_custom_token() {
    let config = TestjigConfig {
        token: "test-spawn-custom-token".to_owned(),
        ..TestjigConfig::default()
    };
    let jig = spawn_in_process(config).await.expect("spawn must succeed");
    assert_eq!(jig.token, "test-spawn-custom-token");

    let client = reqwest::Client::new();
    let url = format!("http://{}/.well-known/jmap", jig.addr);

    // Wrong token → 401.
    let wrong = client
        .get(&url)
        .bearer_auth("wrong-token")
        .send()
        .await
        .expect("HTTP request must complete");
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);

    // Right token → 200.
    let right = client
        .get(&url)
        .bearer_auth(&jig.token)
        .send()
        .await
        .expect("HTTP request must complete");
    assert_eq!(right.status(), StatusCode::OK);
}
