//! WebSocket endpoint per RFC 8887 (bd:JMAP-cf7p.5), served by the
//! [`jmap_websocket::server::Connection`] driver (bd:JMAP-tzcc).
//!
//! Wires `GET /ws` as the JMAP-over-WebSocket subprotocol endpoint.
//! Negotiates the `jmap` subprotocol on the upgrade handshake, maps
//! axum's WebSocket messages to the driver's
//! [`Inbound`] / [`Outbound`], and supplies the two jig-specific parts:
//!
//! - `JigHandler`: dispatches each Request on the jig's dispatcher,
//!   then records state changes for SSE / WS push, the same as `POST
//!   /jmap`; it localizes `RequestError` titles from the handshake's
//!   Accept-Language (draft-atwood-jmap-updates-00 §4.3).
//! - `JigPush`: the push source. Same signal-driven mechanism as
//!   [`crate::sse`] (bd:JMAP-cf7p.9): it waits on a state-change watch
//!   wake or a safety-net poll tick and emits one `StateChange` with the
//!   entries the producer-driven log recorded since its last one.
//!   `pushState` is the decimal event id of the latest entry; a
//!   `WebSocketPushEnable.pushState` the log still covers replays from
//!   there (bd:JMAP-cf7p.12).
//!
//! Message parsing, `requestId` echo, RFC 8887 §4.3.1 error rules,
//! `dataTypes` filtering, push enable / disable and the binary-frame
//! close (1003) are the driver's; see `jmap-websocket`.
//!
//! The upgrade caps each message and frame at `maxSizeRequest`
//! (`MAX_REQUEST_BYTES`), so axum refuses a bigger message while it
//! arrives instead of buffering it; the driver then closes with 1009
//! (Message Too Big, RFC 6455 §7.4.1).
//!
//! # Permission posture
//!
//! Same as the rest of the testjig — single hardcoded account,
//! `CallerCtx` = `()`. The bearer-token middleware ([`crate::auth`])
//! runs before the WS upgrade so the connection is authenticated
//! before any frame is processed.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::{
    extract::{
        State as AxumState,
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    },
    response::Response,
};
use futures::stream::BoxStream;
use futures::{SinkExt as _, StreamExt as _, future};
use jmap_chat_server::websocket::ChatStreamHandler;
use jmap_server::JmapError;
use jmap_types::{Id, JmapRequest, JmapResponse, State, StateChange};
use jmap_websocket::jmap_types::problem::ProblemDetails;
use jmap_websocket::server::{
    Connection, ConnectionConfig, Inbound, Outbound, PushSource, RequestHandler,
};
use jmap_websocket::{WebSocketPushEnable, WsStateChange};

use crate::http::{
    AppState, AppStateInner, MAX_CALLS_IN_REQUEST, MAX_CONCURRENT_REQUESTS, MAX_REQUEST_BYTES,
};
use crate::session;
use crate::sse::{POLL_INTERVAL, TypesFilter};

/// Subprotocol name negotiated on the WS upgrade handshake.
///
/// RFC 8887 §4.2: the client MUST include the value `"jmap"` in the
/// `Sec-WebSocket-Protocol` header, and the server MUST echo the
/// same value back. axum's [`WebSocketUpgrade::protocols`] selects
/// the matching subprotocol from the client's list and sets the
/// response header accordingly; if the client did not request
/// `"jmap"`, the response header is omitted and most WS client
/// libraries abort the handshake on their side.
pub const JMAP_SUBPROTOCOL: &str = jmap_websocket::SUBPROTOCOL;

/// `GET /ws` — RFC 8887 §4 JMAP-over-WebSocket upgrade endpoint.
///
/// Selects the [`JMAP_SUBPROTOCOL`] from the client's offered
/// subprotocol list (if present) and hands the upgraded socket to
/// the module-private `handle_socket` task. The bearer-token
/// middleware runs before this handler, so by the time the upgrade
/// is accepted the caller is known-authenticated.
///
/// A request that is not a valid WebSocket upgrade gets the rejection's
/// status with an RFC 9457 problem details body
/// (draft-atwood-jmap-updates-00 §7.2 names the WebSocket opening
/// handshake).
///
/// The handshake's Accept-Language is the Accept-Language of every Request
/// on the connection (draft-atwood-jmap-updates-00 §4.3): it picks the
/// language of `RequestError` titles. It never changes JMAP data.
pub async fn get_ws(
    AxumState(state): AxumState<AppState>,
    headers: axum::http::HeaderMap,
    upgrade: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
) -> Response {
    let accept_language = crate::http::accept_language(&headers);
    match upgrade {
        Ok(upgrade) => {
            let lang = crate::locale::choose(accept_language);
            // axum reads a whole message before the driver sees it; cap
            // it, and each frame, at the advertised maxSizeRequest so a
            // client cannot make the jig buffer more (axum's own caps are
            // tungstenite's 64 MiB / 16 MiB). A message over the cap is
            // closed with 1009 (see `incoming`).
            upgrade
                .protocols([JMAP_SUBPROTOCOL])
                .max_message_size(MAX_REQUEST_BYTES)
                .max_frame_size(MAX_REQUEST_BYTES)
                .on_upgrade(move |socket| handle_socket(socket, state, lang))
        }
        Err(rejection) => crate::http::problem(
            rejection.status(),
            Some(&rejection.body_text()),
            accept_language,
        ),
    }
}

/// Run the RFC 8887 driver on one upgraded socket until it closes.
async fn handle_socket(socket: WebSocket, state: AppState, lang: crate::locale::Lang) {
    let handler = Arc::new(JigHandler {
        state: Arc::clone(&state.inner),
        lang,
    });
    let push = JigPush {
        state: Arc::clone(&state.inner),
        account: Id::from(session::ACCOUNT_ID),
    };
    // The limits the Session's core capability advertises.
    let config = ConnectionConfig::default()
        .max_calls_in_request(MAX_CALLS_IN_REQUEST)
        .max_size_request(Some(MAX_REQUEST_BYTES))
        .max_concurrent_requests(MAX_CONCURRENT_REQUESTS);

    let (sink, stream) = socket.split();
    let incoming = stream.filter_map(|m| {
        future::ready(match m {
            Ok(m) => inbound(m).map(Ok),
            Err(e) => Some(inbound_error(e)),
        })
    });
    let outgoing = sink.with(|m: Outbound| future::ready(Ok::<_, axum::Error>(outbound(m))));
    futures::pin_mut!(incoming, outgoing);

    // draft-atwood-jmap-chat-wss-00: ChatStreamEnable / ChatStreamDisable
    // and the typing and presence events of the owner's account.
    let chat = ChatStreamHandler::new(
        Arc::clone(&state.inner.chat),
        (),
        [Id::from(session::ACCOUNT_ID)],
        &state.inner.chat_events,
    );

    let connection = Connection::new(handler)
        .with_push(push)
        .with_config(config)
        .with_extension(chat);
    if let Err(e) = connection.run(incoming, outgoing).await {
        // There is no `tracing` dep in this crate; the jig emits
        // diagnostics via `eprintln!` so smoke testing without a
        // subscriber works.
        eprintln!("jmap-testjig ws: {e}");
    }
}

/// The driver's view of an axum message; Ping and Pong are answered by
/// axum and dropped here.
fn inbound(message: Message) -> Option<Inbound> {
    match message {
        Message::Text(text) => Some(Inbound::Text(text.as_str().to_owned())),
        Message::Binary(_) => Some(Inbound::Binary),
        Message::Close(_) => Some(Inbound::Close),
        Message::Ping(_) | Message::Pong(_) => None,
    }
}

/// The driver's view of an axum receive error: a message over the size
/// cap set on the upgrade is [`Inbound::TooBig`], which the driver closes
/// with 1009 (RFC 6455 §7.4.1); any other error stays an error.
///
/// The downcast needs axum and this crate to resolve the same
/// tokio-tungstenite; `tests/ws_size_limit.rs` fails if they split.
fn inbound_error(error: axum::Error) -> Result<Inbound, axum::Error> {
    use tokio_tungstenite::tungstenite::Error as WsError;
    use tokio_tungstenite::tungstenite::error::CapacityError;

    let inner = error.into_inner();
    match inner.downcast::<WsError>() {
        Ok(ws) if matches!(*ws, WsError::Capacity(CapacityError::MessageTooLong { .. })) => {
            Ok(Inbound::TooBig)
        }
        Ok(ws) => Err(axum::Error::new(*ws)),
        Err(other) => Err(axum::Error::new(other)),
    }
}

/// The axum message for a driver message.
fn outbound(message: Outbound) -> Message {
    match message {
        Outbound::Text(text) => Message::Text(text.into()),
        Outbound::Close { code, reason } => Message::Close(Some(CloseFrame {
            code,
            reason: reason.into(),
        })),
        // `Outbound` is non-exhaustive; a future variant the jig does
        // not know ends the connection rather than being dropped.
        _ => Message::Close(None),
    }
}

/// Answers Requests on the jig's dispatcher.
struct JigHandler {
    state: Arc<AppStateInner>,
    lang: crate::locale::Lang,
}

impl RequestHandler for JigHandler {
    async fn handle(&self, request: JmapRequest) -> Result<JmapResponse, JmapError> {
        self.state.check_using(&request)?;
        let response = self
            .state
            .dispatcher
            .dispatch(request, (), State::from(session::STATE))
            .await;
        // Signal-driven SSE / WS push (bd:JMAP-cf7p.9 / .10 / .12):
        // snapshot per-type state, record any changes into the
        // producer-driven event log, and bump the watch counter so the
        // SSE / WS push loops wake and pull the new entries.
        self.state.notify_state_changed().await;
        Ok(response)
    }

    /// draft-atwood-jmap-updates-00 §4.3: the handshake's Accept-Language
    /// picks the title language for the whole connection. A WebSocket
    /// message has no Content-Language header.
    fn prepare_problem(&self, problem: &mut ProblemDetails) {
        if let Some(title) = problem
            .status
            .and_then(|status| crate::locale::title(status, self.lang))
        {
            problem.title = Some(title.to_owned());
        }
    }
}

/// The jig's push source: the producer-driven state change log.
struct JigPush {
    state: Arc<AppStateInner>,
    account: Id,
}

impl PushSource for JigPush {
    /// Mirrors the SSE [`crate::sse`] poll loop: same signal-driven
    /// cadence, same log-pull semantics.
    ///
    /// If `pushState` was supplied (bd:JMAP-cf7p.12) the stream first
    /// replays retained entries past that id. If the log no longer holds
    /// every entry past it, or the token is newer than the log (e.g.
    /// after a jig restart), it starts at "current" without replay and
    /// the client must resync via `Foo/changes`
    /// ([`crate::replay::StateChangeLog::resume_point`]).
    fn subscribe(&mut self, enable: &WebSocketPushEnable) -> BoxStream<'static, WsStateChange> {
        // Only a prefilter: the driver applies RFC 8887 `dataTypes`
        // rules to every notification.
        let filter = TypesFilter::from_data_types(enable.data_types.clone());
        let push_state = enable.push_state.as_deref().and_then(parse_push_state);

        // Subscribe before reading the log so any racing dispatcher wake
        // is captured (cf. SSE poll_loop module docs).
        let mut wake = self.state.subscribe_state_changes();
        wake.mark_unchanged();
        let last_seen = self.state.state_log.resume_point(push_state);
        let mut interval =
            tokio::time::interval_at(tokio::time::Instant::now() + POLL_INTERVAL, POLL_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        let state = Arc::clone(&self.state);
        let account = self.account.clone();
        futures::stream::unfold(
            (state, account, filter, wake, interval, last_seen),
            |(state, account, filter, mut wake, mut interval, last_seen)| async move {
                loop {
                    if let Some((change, max_id)) = pending(&state, &account, last_seen, &filter) {
                        return Some((change, (state, account, filter, wake, interval, max_id)));
                    }
                    tokio::select! {
                        changed = wake.changed() => {
                            if changed.is_err() {
                                return None; // sender dropped — AppState tearing down
                            }
                        }
                        _ = interval.tick() => {}
                    }
                }
            },
        )
        .boxed()
    }
}

/// Parse a wire-format `pushState` token into a `u64` event id.
///
/// The testjig encodes pushState as the decimal `u64` event id
/// assigned by [`crate::replay::StateChangeLog`]. Other server
/// implementations may encode pushState differently; the testjig
/// remains free to evolve this encoding as long as the same encoding
/// is used by both the emit-side and the parse-side.
///
/// Returns `None` for an empty / non-numeric / out-of-range token,
/// in which case the push source treats the subscriber as "no prior
/// state" and starts at the log's current id.
fn parse_push_state(s: &str) -> Option<u64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    t.parse::<u64>().ok()
}

/// The entries past `last_seen` in the state log as one `StateChange`,
/// and the largest event id in it; `None` when there are none.
///
/// The frame carries one state token per type (the latest one in this
/// batch) and a `pushState` naming the largest event id, which clients
/// can echo back on reconnect.
fn pending(
    state: &AppStateInner,
    account: &Id,
    last_seen: u64,
    filter: &TypesFilter,
) -> Option<(WsStateChange, u64)> {
    let entries = state
        .state_log
        .events_since(last_seen, &filter.as_replay_filter());
    if entries.is_empty() {
        return None;
    }
    let mut changes: BTreeMap<String, String> = BTreeMap::new();
    let mut max_id = last_seen;
    for entry in &entries {
        changes.insert(entry.type_name.to_owned(), entry.state_token.clone());
        max_id = max_id.max(entry.event_id);
    }
    Some((build_state_change(account, changes, max_id), max_id))
}

/// The `StateChange` WS frame per RFC 8887 §4.3.5.1 (which extends
/// RFC 8620 §7.1), with `pushState` the decimal `u64` event id
/// (bd:JMAP-cf7p.12).
fn build_state_change(
    account: &Id,
    changes: BTreeMap<String, String>,
    push_state: u64,
) -> WsStateChange {
    let types = changes
        .into_iter()
        .map(|(k, v)| (k, State::from(v)))
        .collect();
    let mut changed = BTreeMap::new();
    changed.insert(account.clone(), types);
    WsStateChange::new(StateChange::new(changed), Some(push_state.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Oracle: RFC 8620 §3.6.1 ("limit" errors MUST carry a "limit"
    /// property naming the limit), RFC 9457 §3.1.4 ("detail" is a
    /// human-readable explanation), RFC 8887 §4.3.4 (`@type` and
    /// `requestId`) and draft-atwood-jmap-updates-00 §4.3 (the handshake
    /// language picks the title). Expected object is hand-written.
    #[test]
    fn request_error_title_follows_the_handshake_language() {
        let mut err = JmapError::limit("maxCallsInRequest");
        err.description = Some("17 calls exceed 16".to_owned());
        let handler = JigHandler {
            state: Arc::clone(&AppState::new().inner),
            lang: crate::locale::Lang::En,
        };
        let mut frame = jmap_websocket::server::request_error(&err, Some("r1".to_owned()));
        handler.prepare_problem(&mut frame.problem);
        assert_eq!(
            serde_json::to_value(&frame).unwrap(),
            json!({
                "@type": "RequestError",
                "type": "urn:ietf:params:jmap:error:limit",
                "status": 400,
                "title": "Bad Request",
                "limit": "maxCallsInRequest",
                "detail": "17 calls exceed 16",
                "requestId": "r1",
            })
        );
        let handler = JigHandler {
            lang: crate::locale::Lang::De,
            ..handler
        };
        let mut problem = ProblemDetails::new("urn:ietf:params:jmap:error:notJSON", 400);
        handler.prepare_problem(&mut problem);
        assert_eq!(problem.title.as_deref(), Some("Ungültige Anfrage"));
    }

    /// Oracle: RFC 8887 §4.3.5.2 — `dataTypes: null` means
    /// "all supported types".
    #[test]
    fn types_filter_from_data_types_null_is_wildcard() {
        let f = TypesFilter::from_data_types(None);
        assert_eq!(f, TypesFilter::Wildcard);
    }

    /// The push source's prefilter maps an empty `dataTypes: []` to
    /// wildcard; the driver then applies RFC 8887 §4.3.5.2 to every
    /// notification (an empty list matches nothing), so the prefilter
    /// only has to never drop a wanted type.
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

    /// Oracle: RFC 8887 §4.3.5.1 + RFC 8620 §7.1 — StateChange frame
    /// shape is `{"@type":"StateChange","changed":{"<acct>":{...}},
    /// "pushState":"<id>"}`. Per bd:JMAP-cf7p.12, the testjig encodes
    /// pushState as the decimal `u64` event id of the latest entry
    /// in the batch; clients echo it on reconnect via
    /// `WebSocketPushEnable.pushState`.
    #[test]
    fn state_change_frame_shape_matches_rfc_8887_section_4_3_5_1() {
        let account = Id::from("acct-1");
        let mut changes = BTreeMap::new();
        changes.insert("Space".to_owned(), "7".to_owned());
        let frame = build_state_change(&account, changes, 42);
        assert_eq!(
            serde_json::to_value(&frame).unwrap(),
            json!({
                "@type": "StateChange",
                "changed": {"acct-1": {"Space": "7"}},
                "pushState": "42",
            })
        );
    }

    /// Oracle: bd:JMAP-cf7p.12 — pushState parser handles missing,
    /// empty, numeric, and non-numeric values per its docstring.
    #[test]
    fn parse_push_state_handles_missing_empty_and_numeric() {
        assert_eq!(parse_push_state(""), None);
        assert_eq!(parse_push_state("   "), None);
        assert_eq!(parse_push_state("42"), Some(42));
        assert_eq!(parse_push_state("0"), Some(0));
        assert_eq!(parse_push_state("abc"), None);
        // u64::MAX + 1 overflows and falls back to None.
        assert_eq!(parse_push_state("99999999999999999999999"), None);
    }
}
