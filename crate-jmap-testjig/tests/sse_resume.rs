//! Integration smoke test for SSE Last-Event-ID replay (bd:JMAP-cf7p.10).
//!
//! Exercises the reconnect path:
//!
//! 1. Spawn the jig.
//! 2. Open `GET /events`, observe a `Space` state event with some id N.
//! 3. Close the SSE response.
//! 4. POST one or more additional `Space/set` mutations through `POST /jmap`
//!    (no SSE connection open during these — the producer-driven log records
//!    them anyway).
//! 5. Reconnect to `GET /events` with `Last-Event-ID: N`.
//! 6. Assert: the testjig immediately replays the post-N events so the
//!    client sees the missed mutations without having to issue
//!    `Foo/changes`.

use std::time::Duration;

use jmap_testjig::{spawn_in_process, TestjigConfig};
use serde_json::{json, Value};

const SSE_WAIT_BUDGET: Duration = Duration::from_secs(5);

#[tokio::test]
async fn last_event_id_replays_missed_changes() {
    let jig = spawn_in_process(TestjigConfig::default())
        .await
        .expect("spawn testjig");
    let token = jig.token.clone();
    let addr = jig.addr;

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .build()
        .expect("reqwest client");

    let api_url = format!("http://{addr}/jmap");
    let events_url = format!("http://{addr}/events?types=*&closeafter=no&ping=0");

    // -- Phase 1: open SSE, mutate once, observe the event id.
    let mut sse1 = client
        .get(&events_url)
        .bearer_auth(&token)
        .send()
        .await
        .expect("SSE request");
    assert!(sse1.status().is_success());

    // Allow the SSE subscriber to register on the watch channel.
    tokio::time::sleep(Duration::from_millis(50)).await;

    space_create(&client, &api_url, &token, "phase-1").await;

    let phase1_event = read_state_event_for(&mut sse1, "Space")
        .await
        .expect("phase-1 state event must arrive");
    let phase1_id: u64 = phase1_event.id.parse().expect("event id is u64");
    assert!(
        phase1_id >= 1,
        "first event id must be >= 1, got {phase1_id}"
    );

    // Drop the first SSE connection.
    drop(sse1);

    // Pause so the previous subscriber's task definitely terminates
    // (its tx is dropped when sse1 drops, ending its loop). This is
    // not strictly required for correctness but keeps the test
    // deterministic against scheduler reorderings.
    tokio::time::sleep(Duration::from_millis(20)).await;

    // -- Phase 2: mutate while no SSE is open. The producer-driven
    //    event log records these regardless.
    space_create(&client, &api_url, &token, "phase-2a").await;
    space_create(&client, &api_url, &token, "phase-2b").await;

    // -- Phase 3: reconnect with Last-Event-ID; expect replay.
    let mut sse2 = client
        .get(&events_url)
        .bearer_auth(&token)
        .header("Last-Event-ID", phase1_id.to_string())
        .send()
        .await
        .expect("SSE reconnect");
    assert!(sse2.status().is_success());

    // The reconnect replay event SHOULD arrive promptly without
    // requiring another POST /jmap to wake the subscriber. RFC 8620
    // §7.3: "the server [...] SHOULD send these changes immediately
    // on connection."
    let replay_event = read_state_event_for(&mut sse2, "Space")
        .await
        .expect("replay state event must arrive on reconnect");

    let replay_id: u64 = replay_event.id.parse().expect("replay id is u64");
    assert!(
        replay_id > phase1_id,
        "replay id ({replay_id}) must be > phase1 id ({phase1_id})"
    );

    let payload: Value = serde_json::from_str(&replay_event.data).expect("replay data is JSON");
    assert_eq!(payload["@type"], "StateChange");
    let space_state = payload["changed"]["testjig-account"]["Space"]
        .as_str()
        .expect("Space state in replay payload");
    assert!(!space_state.is_empty());

    drop(sse2);
    jig.shutdown().await;
}

/// POST a `Space/set` create with a unique name. Returns when the
/// dispatcher has finished and the event log has recorded the change.
async fn space_create(client: &reqwest::Client, url: &str, token: &str, name: &str) {
    let body = json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:chat"],
        "methodCalls": [
            ["Space/set", {
                "accountId": "testjig-account",
                "create": {"new-1": {"name": name}}
            }, "c1"]
        ]
    });
    let resp = client
        .post(url)
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .expect("Space/set request");
    assert_eq!(
        resp.status().as_u16(),
        200,
        "Space/set must dispatch successfully (name={name})"
    );
    let _: Value = resp.json().await.expect("Space/set body parses");
}

/// A single SSE event decoded out of the body stream.
struct ParsedSseEvent {
    id: String,
    data: String,
}

/// Read SSE chunks until a `state` event arrives whose data references
/// the named JMAP type, or until [`SSE_WAIT_BUDGET`] elapses.
///
/// Minimal SSE parser — splits on `\n\n`, reads field lines of the
/// form `field: value`. Same shape as the helper in `sse_smoke.rs`
/// but trimmed to only the fields this test asserts on.
async fn read_state_event_for(
    response: &mut reqwest::Response,
    type_name: &str,
) -> Option<ParsedSseEvent> {
    let deadline = tokio::time::Instant::now() + SSE_WAIT_BUDGET;
    let mut buffer = String::new();

    loop {
        let remaining = deadline.checked_duration_since(tokio::time::Instant::now())?;
        let chunk = match tokio::time::timeout(remaining, response.chunk()).await {
            Ok(Ok(Some(b))) => b,
            Ok(Ok(None)) => return None,
            Ok(Err(e)) => panic!("SSE body read error: {e}"),
            Err(_) => return None,
        };
        let s = std::str::from_utf8(&chunk).expect("SSE bytes UTF-8");
        buffer.push_str(s);

        while let Some(idx) = buffer.find("\n\n") {
            let block = buffer[..idx].to_owned();
            buffer.drain(..idx + 2);

            let mut event_field: Option<String> = None;
            let mut id_field: Option<String> = None;
            let mut data_lines: Vec<String> = Vec::new();
            for line in block.lines() {
                if line.starts_with(':') || line.is_empty() {
                    continue;
                }
                let (field, value) = match line.split_once(':') {
                    Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
                    None => continue,
                };
                match field {
                    "event" => event_field = Some(value.to_owned()),
                    "id" => id_field = Some(value.to_owned()),
                    "data" => data_lines.push(value.to_owned()),
                    _ => {}
                }
            }
            if event_field.as_deref() == Some("state") {
                let data = data_lines.join("\n");
                if data.contains(&format!("\"{type_name}\"")) {
                    return Some(ParsedSseEvent {
                        id: id_field.unwrap_or_default(),
                        data,
                    });
                }
            }
        }
    }
}
