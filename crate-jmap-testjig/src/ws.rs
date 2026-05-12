//! WebSocket endpoint per RFC 8887 (bd:JMAP-cf7p.5).
//!
//! Wires `GET /ws` as the JMAP-over-WebSocket subprotocol endpoint.
//! Negotiates the `jmap` subprotocol on the upgrade handshake; runs a
//! single concurrent recv-and-push loop on each established socket.
//!
//! # Frame envelopes (RFC 8887 §4.3)
//!
//! Every WS message is a single JSON object. The `@type` field is the
//! discriminator that picks which RFC 8887 envelope shape applies.
//!
//! From the client:
//!
//! | `@type` | Body |
//! |---------|------|
//! | `Request` | RFC 8620 §3.3 JmapRequest + optional `id` |
//! | `WebSocketPushEnable` | optional `dataTypes`, optional `pushState` |
//! | `WebSocketPushDisable` | (none) |
//!
//! From the server:
//!
//! | `@type` | Body |
//! |---------|------|
//! | `Response` | RFC 8620 §3.4 JmapResponse + `requestId` (echoed) |
//! | `RequestError` | RFC 7807 Problem Details + `requestId` |
//! | `StateChange` | RFC 8620 §7.1 StateChange + optional `pushState` |
//!
//! Per §4.3.1, any binary frame, fragmented-message that does not
//! coalesce, or text frame that is not valid JSON, is answered with
//! a `RequestError` envelope (when correlation is possible) or
//! ignored (when not).
//!
//! # Push notifications (RFC 8887 §4.3.5)
//!
//! Identical polling-based mechanism as [`crate::sse`]: a tokio
//! interval task ticks at the same `POLL_INTERVAL` (200 ms) as the
//! SSE poller, snapshots per-type state across the 8 reference
//! MemoryBackends, diffs against the previous snapshot, and emits a
//! `StateChange` frame when any tracked type's token advanced.
//!
//! `WebSocketPushEnable` starts the polling task. The handshake's
//! `dataTypes` array (or `null` for "all") feeds the same
//! `TypesFilter` enum the SSE handler uses; subsequent `StateChange`
//! frames are filtered by it. `WebSocketPushDisable` aborts the task.
//! Re-enabling after a disable starts a fresh polling task with a
//! fresh baseline snapshot.
//!
//! The `pushState` token on `PushEnable` is parsed but not honored
//! (filed as bd:JMAP-cf7p.12 — analogous to the SSE Last-Event-ID
//! follow-up at bd:JMAP-cf7p.10).
//!
//! # Permission posture
//!
//! Same as the rest of the testjig — single hardcoded account,
//! CallerCtx = `()`. The bearer-token middleware ([`crate::auth`])
//! runs before the WS upgrade so the connection is authenticated
//! before any frame is processed.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State as AxumState,
    },
    response::Response,
};
use jmap_server::{parse_request, JmapError};
use jmap_types::{Id, State};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::http::{AppState, AppStateInner, MAX_CALLS_IN_REQUEST};
use crate::session;
use crate::sse::{diff_snapshots, snapshot_all_states, TypesFilter, POLL_INTERVAL};

/// Subprotocol name negotiated on the WS upgrade handshake.
///
/// RFC 8887 §4.2: the client MUST include the value `"jmap"` in the
/// `Sec-WebSocket-Protocol` header, and the server MUST echo the
/// same value back. axum's [`WebSocketUpgrade::protocols`] selects
/// the matching subprotocol from the client's list and sets the
/// response header accordingly; if the client did not request
/// `"jmap"`, the response header is omitted and most WS client
/// libraries abort the handshake on their side.
pub const JMAP_SUBPROTOCOL: &str = "jmap";

/// Outbound channel bound between the push poll task and the main WS
/// recv-and-send loop. Same rationale as
/// [`crate::sse::SSE_CHANNEL_BOUND`] — a slow consumer parks the
/// producer rather than dropping frames, and JMAP state-change
/// semantics tolerate the back-pressure because clients can always
/// resync via `/changes`.
const PUSH_CHANNEL_BOUND: usize = 64;

/// `GET /ws` — RFC 8887 §4 JMAP-over-WebSocket upgrade endpoint.
///
/// Selects the [`JMAP_SUBPROTOCOL`] from the client's offered
/// subprotocol list (if present) and hands the upgraded socket to
/// the module-private `handle_socket` task. The bearer-token
/// middleware runs before this handler, so by the time the upgrade
/// is accepted the caller is known-authenticated.
pub async fn get_ws(AxumState(state): AxumState<AppState>, upgrade: WebSocketUpgrade) -> Response {
    upgrade
        .protocols([JMAP_SUBPROTOCOL])
        .on_upgrade(move |socket| handle_socket(socket, state))
}

/// Main per-connection loop. Reads incoming frames, dispatches each
/// per its `@type` discriminator, and forwards push frames from the
/// optional polling task.
///
/// Exits cleanly on receiving a Close frame, on a stream error, on a
/// non-text frame followed by a close, or when the push channel
/// becomes inert (no push task active and no recv task pending).
async fn handle_socket(mut socket: WebSocket, state: AppState) {
    let inner = Arc::clone(&state.inner);
    let account = Id::from(session::ACCOUNT_ID);

    let (push_tx, mut push_rx) = mpsc::channel::<Message>(PUSH_CHANNEL_BOUND);
    let mut push_task: Option<JoinHandle<()>> = None;

    loop {
        tokio::select! {
            // Outbound: forward a push-task-generated StateChange
            // frame to the client. The `push_tx` Sender stays alive
            // for the lifetime of this loop (we keep our own clone),
            // so the channel never closes from sender-drop; a `None`
            // here would mean the runtime tore the channel down
            // under us, in which case the cleanest action is to
            // break out of the WS loop.
            maybe_frame = push_rx.recv() => {
                let Some(frame) = maybe_frame else { break };
                if socket.send(frame).await.is_err() {
                    break;
                }
            }

            // Inbound: handle the next client message.
            maybe_msg = socket.recv() => {
                let Some(Ok(msg)) = maybe_msg else {
                    // None → stream closed; Some(Err) → transport
                    // error. Either way, the WS connection is done.
                    break;
                };

                match msg {
                    Message::Text(text) => {
                        if let Err(reason) = handle_text_frame(
                            &text,
                            Arc::clone(&inner),
                            &account,
                            &mut socket,
                            &push_tx,
                            &mut push_task,
                        ).await {
                            // Any inability to write back means the
                            // socket is gone; exit cleanly.
                            tracing_eprintln(&reason);
                            break;
                        }
                    }
                    Message::Binary(_) => {
                        // RFC 8887 §4.3.1: binary frames may be
                        // ignored or the connection closed with 1003
                        // (Unsupported Data). We close — the testjig
                        // has no use for binary frames.
                        let _ = socket
                            .send(Message::Close(Some(axum::extract::ws::CloseFrame {
                                code: 1003,
                                reason: "binary frames not supported".into(),
                            })))
                            .await;
                        break;
                    }
                    Message::Close(_) => {
                        // Polite client close — exit.
                        break;
                    }
                    // Ping/Pong are auto-handled by axum's WS layer;
                    // we receive them here for visibility but no
                    // action is needed.
                    Message::Ping(_) | Message::Pong(_) => {}
                }
            }
        }
    }

    if let Some(handle) = push_task.take() {
        handle.abort();
    }
}

/// stderr breadcrumb shim — there is no `tracing` dep in this crate,
/// and the test jig deliberately emits diagnostic output via
/// `eprintln!` so smoke testing without a subscriber works. Wrapping
/// the call here keeps the call site clean.
fn tracing_eprintln(reason: &str) {
    eprintln!("jmap-testjig ws: {reason}");
}

/// Decode an inbound text frame and route it through the appropriate
/// RFC 8887 envelope handler.
///
/// Returns `Err(reason)` only when sending the response back to the
/// client failed (the socket is gone); in that case the caller breaks
/// out of the recv loop.
async fn handle_text_frame(
    text: &str,
    state: Arc<AppStateInner>,
    account: &Id,
    socket: &mut WebSocket,
    push_tx: &mpsc::Sender<Message>,
    push_task: &mut Option<JoinHandle<()>>,
) -> Result<(), String> {
    let value: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => {
            return send_request_error(socket, JmapError::not_json(), None).await;
        }
    };

    // The `@type` discriminator is at the top level. A missing or
    // non-string value is a notRequest per RFC 8887 §4.3.1.
    let request_id = value.get("id").and_then(Value::as_str).map(str::to_owned);
    let envelope = value.get("@type").and_then(Value::as_str).unwrap_or("");

    match envelope {
        "Request" => handle_request_frame(value, state, request_id, socket).await,
        "WebSocketPushEnable" => {
            handle_push_enable(value, state, account.clone(), push_tx.clone(), push_task);
            Ok(())
        }
        "WebSocketPushDisable" => {
            handle_push_disable(push_task);
            Ok(())
        }
        _ => send_request_error(socket, JmapError::not_request(), request_id).await,
    }
}

/// Dispatch a `Request` envelope and emit a matching `Response` or
/// `RequestError` envelope on the same socket.
async fn handle_request_frame(
    value: Value,
    state: Arc<AppStateInner>,
    request_id: Option<String>,
    socket: &mut WebSocket,
) -> Result<(), String> {
    let request = match parse_request(value, MAX_CALLS_IN_REQUEST) {
        Ok(r) => r,
        Err(err) => return send_request_error(socket, err, request_id).await,
    };

    let mut response = state
        .dispatcher
        .dispatch(request, (), State::from(session::STATE))
        .await;

    // RFC 8887 §4.3.3 adds two fields to the wire JmapResponse: a
    // fixed `@type: "Response"` discriminator and the echoed
    // `requestId` (if the request carried an `id`). Both ride on the
    // workspace's extras-preservation `extra` field — see
    // jmap-types::wire::JmapResponse.
    response
        .extra
        .insert("@type".to_owned(), Value::String("Response".to_owned()));
    if let Some(req_id) = request_id {
        response
            .extra
            .insert("requestId".to_owned(), Value::String(req_id));
    }
    let body =
        serde_json::to_string(&response).map_err(|e| format!("serialize JmapResponse: {e}"))?;
    socket
        .send(Message::Text(body.into()))
        .await
        .map_err(|e| format!("send Response frame: {e}"))
}

/// Build and send an RFC 8887 §4.3.4 RequestError envelope.
async fn send_request_error(
    socket: &mut WebSocket,
    err: JmapError,
    request_id: Option<String>,
) -> Result<(), String> {
    // RFC 7807 Problem Details + RFC 8887 extension fields. The
    // testjig's POST /jmap path uses jmap-server's
    // `RequestError::into_response` to assemble this shape, but that
    // helper produces an HTTP response (with status + body); over
    // WebSocket we only need the JSON body, with `@type` and the
    // optional `requestId` added on top.
    let mut body = json!({
        "@type": "RequestError",
        "type": format!("urn:ietf:params:jmap:error:{}", err.error_type),
        "status": jmap_server::error_status(&err).as_u16(),
    });
    if err.error_type == "limit" {
        // RFC 8620 §3.6.1: `limit` errors MUST carry a `limit`
        // property naming the exceeded limit. The convention in
        // jmap-server's JmapError is to stash that in `description`.
        if let Some(name) = err.description.as_deref() {
            body["limit"] = Value::String(name.to_owned());
        }
    } else if let Some(detail) = err.description.as_deref() {
        body["detail"] = Value::String(detail.to_owned());
    }
    if let Some(req_id) = request_id {
        body["requestId"] = Value::String(req_id);
    }
    let txt = serde_json::to_string(&body).map_err(|e| format!("serialize RequestError: {e}"))?;
    socket
        .send(Message::Text(txt.into()))
        .await
        .map_err(|e| format!("send RequestError frame: {e}"))
}

/// Parse RFC 8887 §4.3.5.2 `WebSocketPushEnable` arguments.
///
/// Fields not relevant to the testjig MVP (`pushState`) are parsed
/// but not used — see bd:JMAP-cf7p.12.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PushEnable {
    /// RFC 8887 §4.3.5.2: a list of data type names the client is
    /// interested in. `null` (or missing) means "all types".
    #[serde(default)]
    data_types: Option<Vec<String>>,
    /// RFC 8887 §4.3.5.2: an opaque server-issued token from a prior
    /// `StateChange` event the client wants to resume from. The
    /// testjig MVP does not implement replay; the field is parsed
    /// for forward-compat and ignored.
    #[serde(default)]
    #[allow(dead_code)] // see bd:JMAP-cf7p.12 follow-up
    push_state: Option<String>,
}

/// Spawn a polling task that pushes `StateChange` frames on every
/// detected per-type state advance.
///
/// If a push task is already running (the client re-sent
/// `WebSocketPushEnable` without an intervening `WebSocketPushDisable`),
/// the existing task is aborted and replaced with a fresh one that
/// snapshots a new baseline. This matches the spec's silent-replace
/// semantics — RFC 8887 §4.3.5.2 does not forbid re-enabling.
fn handle_push_enable(
    value: Value,
    state: Arc<AppStateInner>,
    account: Id,
    push_tx: mpsc::Sender<Message>,
    push_task: &mut Option<JoinHandle<()>>,
) {
    // Pull args from the envelope. Parse failure (e.g. dataTypes is
    // an object instead of an array) maps to the default (no filter
    // / wildcard) — we deliberately do not emit a RequestError here
    // because PushEnable is a control message, not a Request, and
    // RFC 8887 §4.3.1 says the receiver MAY ignore malformed
    // non-Request frames.
    let args: PushEnable = serde_json::from_value(value).unwrap_or_default();
    let filter = TypesFilter::from_data_types(args.data_types);

    // Replace any existing push task before spawning a fresh one.
    if let Some(existing) = push_task.take() {
        existing.abort();
    }

    let handle = tokio::spawn(push_loop(state, account, filter, push_tx));
    *push_task = Some(handle);
}

/// Stop an in-flight push polling task on `WebSocketPushDisable`.
///
/// No-op if no task is running. Per RFC 8887 §4.3.5.3, the server
/// stops sending `StateChange` frames after this; the client is
/// expected to resync via `/changes` if it needs to recover state
/// changes that occurred between disable and a subsequent enable.
fn handle_push_disable(push_task: &mut Option<JoinHandle<()>>) {
    if let Some(handle) = push_task.take() {
        handle.abort();
    }
}

/// Polling task that pushes `StateChange` frames on diff. Mirrors
/// the SSE [`crate::sse`] poller — same cadence, same snapshot diff
/// algorithm, same filter-by-types semantics — but emits text WS
/// frames instead of SSE events.
///
/// Terminates when the `push_tx` send fails (the main WS task ended,
/// either because the client disconnected or PushDisable aborted
/// this task).
async fn push_loop(
    state: Arc<AppStateInner>,
    account: Id,
    filter: TypesFilter,
    push_tx: mpsc::Sender<Message>,
) {
    let mut previous: BTreeMap<&'static str, String> = snapshot_all_states(&state, &account).await;

    let mut interval = tokio::time::interval(POLL_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // Drop the first immediate tick — the baseline snapshot above is
    // what we want, not a redundant snapshot now.
    interval.tick().await;

    loop {
        interval.tick().await;
        let current = snapshot_all_states(&state, &account).await;
        let changes = diff_snapshots(&previous, &current, &filter);
        previous = current;
        if changes.is_empty() {
            continue;
        }
        let body = build_state_change_frame(&account, &changes);
        let serialized = match serde_json::to_string(&body) {
            Ok(s) => s,
            Err(_) => return, // unreachable for JSON-safe primitives
        };
        if push_tx
            .send(Message::Text(serialized.into()))
            .await
            .is_err()
        {
            return; // receiver gone — main WS task ended
        }
    }
}

/// Build the JSON body for a `StateChange` WS frame per RFC 8887
/// §4.3.5.1 (which extends RFC 8620 §7.1).
///
/// The `pushState` field is intentionally omitted — the testjig MVP
/// does not implement state-resume support (bd:JMAP-cf7p.12). When
/// present, clients can use it to recover state changes missed
/// across reconnects; without it, clients fall back to issuing
/// `Foo/changes` after reconnect.
fn build_state_change_frame(account: &Id, changes: &BTreeMap<String, String>) -> Value {
    json!({
        "@type": "StateChange",
        "changed": {
            account.as_ref(): changes,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle: RFC 8887 §4.3.5.2 — `dataTypes: null` means
    /// "all supported types".
    #[test]
    fn types_filter_from_data_types_null_is_wildcard() {
        let f = TypesFilter::from_data_types(None);
        assert_eq!(f, TypesFilter::Wildcard);
    }

    /// Oracle: an empty `dataTypes: []` array is functionally
    /// equivalent to `null` — no type names listed. We map both to
    /// wildcard; the alternative ("never match") is an attractive
    /// trap.
    #[test]
    fn types_filter_from_data_types_empty_is_wildcard() {
        let f = TypesFilter::from_data_types(Some(Vec::new()));
        assert_eq!(f, TypesFilter::Wildcard);
    }

    /// Oracle: RFC 8887 §4.3.5.2 — non-empty `dataTypes` is the
    /// filter set.
    #[test]
    fn types_filter_from_data_types_list_is_only() {
        let f = TypesFilter::from_data_types(Some(vec!["Email".to_owned(), "Mailbox".to_owned()]));
        let TypesFilter::Only(set) = f else {
            panic!("expected Only");
        };
        assert!(set.contains("Email"));
        assert!(set.contains("Mailbox"));
        assert_eq!(set.len(), 2);
    }

    /// Oracle: RFC 8887 §4.3.5.2 — the `PushEnable` envelope can omit
    /// `dataTypes` (the `null` case) and `pushState` (optional).
    #[test]
    fn push_enable_parses_minimal_envelope() {
        let v = json!({"@type": "WebSocketPushEnable"});
        let parsed: PushEnable = serde_json::from_value(v).expect("parse PushEnable");
        assert!(parsed.data_types.is_none());
        assert!(parsed.push_state.is_none());
    }

    /// Oracle: RFC 8887 §4.3.5.2 — `dataTypes` populates the typed
    /// filter list; `pushState` is parsed (and currently ignored).
    #[test]
    fn push_enable_parses_full_envelope() {
        let v = json!({
            "@type": "WebSocketPushEnable",
            "dataTypes": ["Email", "Mailbox"],
            "pushState": "opaque-token-42"
        });
        let parsed: PushEnable = serde_json::from_value(v).expect("parse PushEnable");
        assert_eq!(
            parsed.data_types,
            Some(vec!["Email".to_owned(), "Mailbox".to_owned()])
        );
        assert_eq!(parsed.push_state.as_deref(), Some("opaque-token-42"));
    }

    /// Oracle: RFC 8887 §4.3.5.1 + RFC 8620 §7.1 — StateChange frame
    /// shape is `{"@type":"StateChange","changed":{"<acct>":{...}}}`.
    /// `pushState` is optional per §4.3.5.1; the testjig MVP omits it.
    #[test]
    fn state_change_frame_shape_matches_rfc_8887_section_4_3_5_1() {
        let account = Id::from("acct-1");
        let mut changes = BTreeMap::new();
        changes.insert("Space".to_owned(), "7".to_owned());
        let body = build_state_change_frame(&account, &changes);
        assert_eq!(body["@type"], "StateChange");
        assert_eq!(body["changed"]["acct-1"]["Space"], "7");
        assert!(
            body.get("pushState").is_none(),
            "testjig MVP omits pushState per bd:JMAP-cf7p.12; got {body}"
        );
    }
}
