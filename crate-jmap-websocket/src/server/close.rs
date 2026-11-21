//! Closing a running connection from the deployment: [`CloseHandle`] and
//! [`CloseCode`].
//!
//! RFC 8887 §4.2: "If the authentication credentials for the user expire,
//! the server can either treat subsequent requests as if they are
//! unauthenticated or close the WebSocket connection. In the latter case,
//! the server MAY send a Close frame with a status code of 1008 (Policy
//! Violation)". The JMAP Chat, VTC and Scene WebSocket drafts make 1008 a
//! SHOULD (draft-atwood-jmap-chat-wss-00 §Authentication and the matching
//! sections of the VTC and Scene drafts).

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::Notify;

/// The longest Close reason, in bytes: a control frame payload is at most
/// 125 bytes (RFC 6455 §5.5), two of them the status code.
pub const MAX_CLOSE_REASON_LEN: usize = 123;

/// A WebSocket Close status code an endpoint may send (RFC 6455 §7.4).
///
/// Only codes a peer accepts can be made: 1000 to 1003 and 1007 to 1013
/// (RFC 6455 §7.4.1 and the IANA WebSocket Close Code Number Registry),
/// and 3000 to 4999 (§7.4.2: registered libraries and frameworks, and
/// private use). 1004, 1005, 1006 and 1015 must not be sent; 1014 and
/// 1016 to 2999 are rejected by common client libraries (tungstenite
/// treats them as a protocol error).
///
/// ```
/// use jmap_websocket::server::CloseCode;
///
/// assert_eq!(CloseCode::POLICY_VIOLATION.as_u16(), 1008);
/// assert_eq!(CloseCode::new(4001).map(u16::from), Some(4001));
/// assert_eq!(CloseCode::new(1006), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CloseCode(u16);

impl CloseCode {
    /// 1000 Normal Closure.
    pub const NORMAL: Self = Self(1000);
    /// 1001 Going Away, for example a server shutting down.
    pub const GOING_AWAY: Self = Self(1001);
    /// 1008 Policy Violation: the code RFC 8887 §4.2 names for expired
    /// credentials.
    pub const POLICY_VIOLATION: Self = Self(1008);

    /// The close code `code`, or `None` when an endpoint may not send it
    /// (see the [type docs](Self)).
    pub const fn new(code: u16) -> Option<Self> {
        match code {
            1000..=1003 | 1007..=1013 | 3000..=4999 => Some(Self(code)),
            _ => None,
        }
    }

    /// The numeric status code.
    pub const fn as_u16(self) -> u16 {
        self.0
    }
}

impl From<CloseCode> for u16 {
    fn from(code: CloseCode) -> u16 {
        code.0
    }
}

impl fmt::Display for CloseCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Where a [`Connection`](super::Connection) is in its life, as its
/// [`CloseHandle`]s see it.
#[derive(Debug)]
enum State {
    /// Not closed yet (`run` may not have started).
    Open,
    /// [`CloseHandle::close`] asked for this Close.
    Requested(CloseCode, String),
    /// `run` returned.
    Ended,
}

/// The state one connection shares with its [`CloseHandle`]s.
#[derive(Debug)]
pub(super) struct Shared {
    state: Mutex<State>,
    changed: Notify,
}

impl Shared {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::Open),
            changed: Notify::new(),
        })
    }

    // No code path panics while holding the lock, so a poisoned lock
    // still holds a consistent state.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The Close requested so far, if any.
    pub(super) fn requested_now(&self) -> Option<(CloseCode, String)> {
        match &*self.lock() {
            State::Requested(code, reason) => Some((*code, reason.clone())),
            State::Open | State::Ended => None,
        }
    }

    /// The Close to send, once one is requested; pending until then.
    pub(super) async fn requested(&self) -> (CloseCode, String) {
        loop {
            // Made before the check, so a `close` between the check and
            // the await still wakes it (`notify_waiters` reaches every
            // `Notified` that exists).
            let changed = self.changed.notified();
            if let Some(requested) = self.requested_now() {
                return requested;
            }
            changed.await;
        }
    }

    /// `run` returned: later [`CloseHandle::close`] calls do nothing.
    pub(super) fn end(&self) {
        *self.lock() = State::Ended;
    }
}

/// Closes a running [`Connection`](super::Connection) from outside it,
/// for example when the credentials of the opening handshake expire (RFC
/// 8887 §4.2). Get one with
/// [`Connection::close_handle`](super::Connection::close_handle) before
/// calling [`run`](super::Connection::run); clone it freely and send it
/// to another task.
///
/// [`close`](Self::close) makes `run` send a Close frame with the given
/// code and reason, run the same shutdown as any other end (push
/// subscription dropped, extension queue closed,
/// [`ExtensionHandler::close`](super::ExtensionHandler::close) called),
/// and return `Ok(())`. `run` does not wait for the client's answering
/// Close; the transport (for example
/// `tungstenite::serve`) drops the socket.
///
/// Nothing queued is sent ahead of the Close: Responses of Requests still
/// running, queued extension messages and push notifications not yet
/// written are dropped. A message `run` is writing when `close` is called
/// is finished first. After credentials expire this is what the drafts
/// ask: "A server that does not close the connection when credentials
/// expire SHOULD NOT deliver further ephemeral events on that connection"
/// (draft-atwood-jmap-chat-wss-00).
///
/// ```
/// use std::sync::Arc;
/// use std::time::Duration;
/// use jmap_websocket::jmap_server::Dispatcher;
/// use jmap_websocket::jmap_types::State;
/// use jmap_websocket::server::{CloseCode, Connection, DispatcherHandler};
///
/// # async fn serve(ws: jmap_websocket::tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
/// #     dispatcher: Arc<Dispatcher<()>>, expires_in: Duration)
/// #     -> Result<(), jmap_websocket::server::ConnectionError> {
/// let handler = Arc::new(DispatcherHandler::new(dispatcher, (), || State::from("s0")));
/// let connection = Connection::new(handler);
/// let close = connection.close_handle();
/// // The credentials of the opening handshake expire after `expires_in`.
/// tokio::spawn(async move {
///     tokio::time::sleep(expires_in).await;
///     close.close(CloseCode::POLICY_VIOLATION, "credentials expired");
/// });
/// jmap_websocket::tungstenite::serve(connection, ws).await
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct CloseHandle {
    shared: Arc<Shared>,
}

impl CloseHandle {
    pub(super) fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }

    /// Ask the connection to close with `code` and `reason`.
    ///
    /// `reason` longer than [`MAX_CLOSE_REASON_LEN`] bytes is cut to the
    /// longest prefix within it that ends on a character boundary, so the
    /// Close frame stays a valid control frame (RFC 6455 §5.5).
    ///
    /// Returns `true` when this call is the request that `run` acts on.
    /// Returns `false`, and does nothing, when an earlier `close` (through
    /// any clone) asked already (the first request wins), or `run` has
    /// returned (the client closed, the stream ended, or it failed).
    /// Called before `run` starts, the Close is the first thing `run`
    /// sends. `true` is not a promise of delivery: the client may close,
    /// or the socket fail, at the same moment.
    pub fn close(&self, code: CloseCode, reason: impl Into<String>) -> bool {
        let mut state = self.shared.lock();
        if !matches!(*state, State::Open) {
            return false;
        }
        *state = State::Requested(code, truncate_reason(reason.into()));
        drop(state);
        self.shared.changed.notify_waiters();
        true
    }
}

/// `reason` cut to at most [`MAX_CLOSE_REASON_LEN`] bytes, on a character
/// boundary.
fn truncate_reason(mut reason: String) -> String {
    if reason.len() > MAX_CLOSE_REASON_LEN {
        let mut end = MAX_CLOSE_REASON_LEN;
        while !reason.is_char_boundary(end) {
            end -= 1;
        }
        reason.truncate(end);
    }
    reason
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6455 §7.4.1 defines 1000-1003 and 1007-1011, and says 1004 is
    /// reserved and 1005, 1006, 1015 "MUST NOT be set as a status code in
    /// a Close control frame by an endpoint"; §7.4.2 gives 0-999 no use
    /// and 3000-4999 to libraries and applications.
    #[test]
    fn only_sendable_codes_are_made() {
        for ok in [
            1000, 1001, 1002, 1003, 1007, 1008, 1011, 1012, 1013, 3000, 4999,
        ] {
            assert_eq!(CloseCode::new(ok).map(CloseCode::as_u16), Some(ok), "{ok}");
        }
        for bad in [
            0,
            999,
            1004,
            1005,
            1006,
            1014,
            1015,
            1016,
            2999,
            5000,
            u16::MAX,
        ] {
            assert_eq!(CloseCode::new(bad), None, "{bad}");
        }
        assert_eq!(CloseCode::POLICY_VIOLATION.as_u16(), 1008);
        assert_eq!(CloseCode::GOING_AWAY.as_u16(), 1001);
        assert_eq!(CloseCode::NORMAL.as_u16(), 1000);
        assert_eq!(CloseCode::POLICY_VIOLATION.to_string(), "1008");
    }

    /// RFC 6455 §5.5: "All control frames MUST have a payload length of
    /// 125 bytes or less"; §5.5.1: the body is a 2-byte code then the
    /// reason, so the reason has 123 bytes.
    #[test]
    fn reason_is_cut_to_123_bytes_on_a_char_boundary() {
        assert_eq!(MAX_CLOSE_REASON_LEN, 125 - 2);
        let fits = "a".repeat(123);
        assert_eq!(truncate_reason(fits.clone()), fits);
        assert_eq!(truncate_reason("a".repeat(124)), fits);
        // 122 one-byte characters, then a 2-byte one straddling byte 123.
        let straddles = format!("{}é", "a".repeat(122));
        assert_eq!(straddles.len(), 124);
        assert_eq!(truncate_reason(straddles), "a".repeat(122));
        // 4-byte characters: 30 of them are 120 bytes, a 31st would be 124.
        let wide = "\u{1F512}".repeat(40);
        assert_eq!(truncate_reason(wide), "\u{1F512}".repeat(30));
    }

    #[test]
    fn first_close_wins_and_none_after_the_end() {
        let shared = Shared::new();
        let a = CloseHandle::new(Arc::clone(&shared));
        let b = a.clone();
        assert!(a.close(CloseCode::POLICY_VIOLATION, "expired"));
        assert!(!b.close(CloseCode::GOING_AWAY, "shutdown"));
        assert!(matches!(
            &*shared.lock(),
            State::Requested(CloseCode::POLICY_VIOLATION, r) if r == "expired"
        ));
        shared.end();
        assert!(!a.close(CloseCode::NORMAL, ""));

        let ended = Shared::new();
        ended.end();
        assert!(!CloseHandle::new(ended).close(CloseCode::NORMAL, ""));
    }

    /// A `requested` that is already waiting wakes up on `close`.
    #[tokio::test]
    async fn requested_wakes_on_close() {
        let shared = Shared::new();
        let handle = CloseHandle::new(Arc::clone(&shared));
        let waiter = tokio::spawn({
            let shared = Arc::clone(&shared);
            async move { shared.requested().await }
        });
        tokio::task::yield_now().await;
        assert!(handle.close(CloseCode::POLICY_VIOLATION, "bye"));
        let got = tokio::time::timeout(std::time::Duration::from_secs(5), waiter)
            .await
            .expect("woken")
            .unwrap();
        assert_eq!(got, (CloseCode::POLICY_VIOLATION, "bye".to_owned()));
    }
}
