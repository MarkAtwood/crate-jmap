//! Typed JMAP Chat method wrappers — response types, `Patch<T>`, SessionClient,
//! input/patch structs, constants, and helpers.
//!
//! Response types mirror RFC 8620 standard shapes (§5.1 /get, §5.5 /query,
//! §5.2 /changes, §5.3 /set). Method implementations live in sub-modules and
//! operate on `SessionClient`.

pub mod contact;
pub mod misc;
pub mod space_ban;
pub mod space_invite;

pub mod blob;
pub mod custom_emoji;
pub mod quota;

use std::collections::HashMap;

use serde::Deserialize;

use jmap_types::Id;

// ---------------------------------------------------------------------------
// Response types (RFC 8620 §5)
// ---------------------------------------------------------------------------
//
// Re-exported from `jmap-types::methods` so all `jmap-*-client` crates share
// one canonical set of /get, /set, /changes, /query, /queryChanges shapes.
// The wire format is identical to the previous local definitions.
//
// JMAP Chat extends `SetError` with a `serverRetryAfter` field for slow-mode
// rate limiting. The base `SetError` captures unknown extension fields in
// `extra` via `#[serde(flatten)]`; the [`server_retry_after`] free function
// at the bottom of this module reads that field.

pub use jmap_types::{
    AddedItem, ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SetError,
    SetResponse,
};

/// Response to a `PushSubscription/set` create call (RFC 8620 §7.2).
///
/// `account_id` is always `null` for PushSubscription objects (they are not
/// account-scoped). `Option<Id>` handles both the null case and servers that
/// echo the session accountId anyway.
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushSubscriptionCreateResponse {
    /// The account this response refers to. Always `None` for `PushSubscription`
    /// (not account-scoped); preserved as `Option<Id>` for servers that echo it.
    #[serde(default)]
    pub account_id: Option<Id>,
    /// Successfully created subscriptions, keyed by the caller-supplied creation key.
    pub created: Option<HashMap<String, serde_json::Value>>,
    /// Creation failures, keyed by the caller-supplied creation key.
    #[serde(default)]
    pub not_created: Option<HashMap<String, SetError>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    ///
    /// **Constraint**: keys in `extra` MUST NOT collide with the
    /// typed-field wire names above (the camelCase spelling — e.g.
    /// `"accountId"`, `"ids"`, `"properties"`, `"blobIds"`,
    /// `"fromAccountId"`, etc.). On collision the typed-field value
    /// wins on the wire and the `extra` value is silently dropped at
    /// serialization. Place vendor extensions under vendor-prefixed
    /// keys (e.g. `"acmeCorpFoo"`) to avoid the collision class.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Response to a `Chat/typing` call (JMAP Chat §Chat/typing).
///
/// The server echoes only `accountId`.
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypingResponse {
    /// The account this response refers to.
    pub account_id: Id,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    ///
    /// **Constraint**: keys in `extra` MUST NOT collide with the
    /// typed-field wire names above (the camelCase spelling — e.g.
    /// `"accountId"`, `"ids"`, `"properties"`, `"blobIds"`,
    /// `"fromAccountId"`, etc.). On collision the typed-field value
    /// wins on the wire and the `extra` value is silently dropped at
    /// serialization. Place vendor extensions under vendor-prefixed
    /// keys (e.g. `"acmeCorpFoo"`) to avoid the collision class.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Response to a `Space/join` call (JMAP Chat §Space/join).
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceJoinResponse {
    /// The account this response refers to.
    pub account_id: Id,
    /// The JMAP id of the Space the caller is now a member of.
    pub space_id: Id,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    ///
    /// **Constraint**: keys in `extra` MUST NOT collide with the
    /// typed-field wire names above (the camelCase spelling — e.g.
    /// `"accountId"`, `"ids"`, `"properties"`, `"blobIds"`,
    /// `"fromAccountId"`, etc.). On collision the typed-field value
    /// wins on the wire and the `extra` value is silently dropped at
    /// serialization. Place vendor extensions under vendor-prefixed
    /// keys (e.g. `"acmeCorpFoo"`) to avoid the collision class.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Patch<T>: three-way update value for nullable fields
// ---------------------------------------------------------------------------

/// Three-way patch value for nullable JMAP fields.
///
/// - `Keep` (default): the field is omitted from the patch — server leaves it unchanged.
/// - `Set(v)`: the field is included with value `v`.
/// - `Clear`: the field is included as JSON `null` (clears the server-side value).
///
/// Use `Patch::from(v)` to construct `Set(v)`. Use `Default::default()` or
/// `Patch::Keep` to leave the field unchanged. Use `Patch::Clear` to set a
/// nullable field to null explicitly.
///
/// # Serde usage
///
/// Fields of type `Patch<T>` **must** carry both attributes:
/// ```ignore
/// #[serde(default, skip_serializing_if = "Patch::is_keep")]
/// pub my_field: Patch<String>,
/// ```
/// - `default`: absent JSON key → `Patch::Keep` (no change).
/// - `skip_serializing_if`: omits the key from the output when the value is `Keep`.
///
/// Both attributes are required. Without `skip_serializing_if`, `Patch::Keep`
/// serialises as a runtime error. Without `default`, an absent JSON key
/// silently deserialises as `Patch::Clear` rather than `Patch::Keep` — a
/// silent semantic corruption, not a parse error. See the doctest below
/// for both failure modes.
///
/// # Deserialization
///
/// `Patch::Keep` is **not reachable from JSON deserialization**. The custom
/// `Deserialize` impl maps JSON `null` → `Clear` and a JSON value → `Set(v)`.
/// An absent key (via `#[serde(default)]`) produces `Keep` via `Default`.
///
/// # Failure modes if the attributes are wrong
///
/// The two `#[serde(...)]` attributes above are not enforced by the type
/// system. Forgetting either of them produces a failure far from the
/// declaration site — the doctest below documents what each failure looks
/// like so downstream callers building their own `*Patch` structs can
/// recognise them in production.
///
/// ```rust
/// use jmap_chat_client::Patch;
/// use serde::{Deserialize, Serialize};
///
/// // ── Correct: both attributes present ────────────────────────────────
/// #[derive(Serialize, Deserialize, PartialEq, Debug)]
/// struct GoodPatch {
///     #[serde(default, skip_serializing_if = "Patch::is_keep")]
///     name: Patch<String>,
/// }
///
/// // `Patch::Keep` is the Default; absent JSON key deserialises to Keep.
/// let v: GoodPatch = serde_json::from_str("{}").unwrap();
/// assert_eq!(v.name, Patch::Keep);
/// // `Patch::Keep` is omitted from the serialised output.
/// assert_eq!(serde_json::to_string(&v).unwrap(), "{}");
///
/// // `Patch::Set(v)` serialises as `"name": v` and round-trips back.
/// let v = GoodPatch { name: Patch::Set("Alice".into()) };
/// let s = serde_json::to_string(&v).unwrap();
/// assert_eq!(s, r#"{"name":"Alice"}"#);
/// assert_eq!(serde_json::from_str::<GoodPatch>(&s).unwrap(), v);
///
/// // `Patch::Clear` serialises as `"name": null` and round-trips back.
/// let v = GoodPatch { name: Patch::Clear };
/// let s = serde_json::to_string(&v).unwrap();
/// assert_eq!(s, r#"{"name":null}"#);
/// assert_eq!(serde_json::from_str::<GoodPatch>(&s).unwrap(), v);
///
/// // ── Wrong: missing `default` ────────────────────────────────────────
/// // Absent JSON key silently deserialises as `Patch::Clear` — NOT
/// // `Patch::Keep` and NOT a missing-field error. This is the most
/// // dangerous failure mode of the three because it surfaces no error
/// // at all: a server response that omits an unchanged field is
/// // interpreted by the client as "the server cleared this field".
/// //
/// // The mechanism: `Patch<T>`'s custom `Deserialize` impl calls
/// // `Option::<T>::deserialize(d)`, and serde treats `Option`-shaped
/// // deserializers as accepting absence by returning `None`. The custom
/// // impl then maps `None` to `Patch::Clear`. `#[serde(default)]` is
/// // what tells serde to skip calling the deserialize at all on absent
/// // keys and use `Default::default()` (`Patch::Keep`) instead.
/// #[derive(Deserialize, PartialEq, Debug)]
/// struct MissingDefault {
///     #[serde(skip_serializing_if = "Patch::is_keep")]
///     name: Patch<String>,
/// }
/// let v: MissingDefault = serde_json::from_str("{}").unwrap();
/// assert_eq!(v.name, Patch::Clear, "without #[serde(default)], absent key silently becomes Clear");
///
/// // ── Wrong: missing `skip_serializing_if` ───────────────────────────
/// // The default `Patch::Keep` panics at serialise time with a custom
/// // error from the `Serialize` impl in this crate. This surfaces every
/// // time a caller builds a patch and leaves a field at its `Keep`
/// // default — i.e. every partial update.
/// #[derive(Serialize)]
/// struct MissingSkip {
///     #[serde(default)]
///     name: Patch<String>,
/// }
/// let v = MissingSkip { name: Patch::Keep };
/// let err = serde_json::to_string(&v).unwrap_err();
/// assert!(
///     err.to_string().contains("Patch::Keep cannot be serialized"),
///     "expected Patch::Keep serialisation error, got: {err}"
/// );
/// ```
///
/// # Closed enum (no `#[non_exhaustive]`)
///
/// `Patch` is deliberately **not** marked `#[non_exhaustive]`. The three
/// variants exhaustively model JMAP's tri-state patch semantic
/// (RFC 8620 §5.3: absent key = no change; value = set; JSON `null` =
/// clear), and no future spec extension is anticipated. Callers may
/// `match` on `Patch<T>` without a wildcard arm. Every other public
/// enum in this crate carries `#[non_exhaustive]`; this one is the
/// principled exception per bd:JMAP-26di.48.
#[derive(Debug, Default, Clone, PartialEq)]
pub enum Patch<T> {
    /// Omit the field from the patch — server leaves it unchanged.
    #[default]
    Keep,
    /// Include the field with value `T`.
    Set(T),
    /// Include the field as JSON `null` (clears the server-side value).
    Clear,
}

impl<T> Patch<T> {
    /// Returns `true` if this is `Patch::Keep` (field should be omitted from serialization).
    pub fn is_keep(&self) -> bool {
        matches!(self, Patch::Keep)
    }
}

impl<T> From<T> for Patch<T> {
    fn from(v: T) -> Self {
        Patch::Set(v)
    }
}

impl<T: serde::Serialize> Patch<T> {
    /// Returns `None` when `Keep` (omit key from patch),
    /// `Some(Value::Null)` when `Clear`, or `Some(serialized_value)` when `Set`.
    ///
    /// Crate-internal helper for the `*_set` / `*_update` builders. Not
    /// exposed publicly because (a) the return type would leak
    /// `serde_json::Error` into the public SemVer surface (per
    /// bd:JMAP-26di.35) and (b) external callers construct `Patch<T>`
    /// via `Patch::from(v)` / `Patch::Clear` and let serde + the
    /// `#[serde(skip_serializing_if = "Patch::is_keep")]` attribute
    /// handle wire serialization — they have no reason to call this
    /// helper directly.
    pub(crate) fn map_entry(&self) -> Result<Option<serde_json::Value>, serde_json::Error> {
        match self {
            Patch::Keep => Ok(None),
            Patch::Clear => Ok(Some(serde_json::Value::Null)),
            Patch::Set(v) => serde_json::to_value(v).map(Some),
        }
    }
}

impl<T: serde::Serialize> serde::Serialize for Patch<T> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Patch::Keep => Err(serde::ser::Error::custom(
                "Patch::Keep cannot be serialized; add \
                 #[serde(skip_serializing_if = \"Patch::is_keep\")] to the field",
            )),
            Patch::Clear => s.serialize_none(),
            Patch::Set(v) => v.serialize(s),
        }
    }
}

impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for Patch<T> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // JSON absent (via #[serde(default)]) → Keep (default).
        // JSON null → Clear. JSON value → Set(v).
        Option::<T>::deserialize(d).map(|opt| match opt {
            None => Patch::Clear,
            Some(v) => Patch::Set(v),
        })
    }
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// The call-id embedded in every single-method JMAP request produced
/// by this crate. Pass directly to
/// [`jmap_base_client::extract_response`].
///
/// External callers need this constant when indexing into
/// `SetResponse`-shaped `created` / `notCreated` / `updated` maps
/// returned by methods that use a per-call client id. For example,
/// after `blob_convert` callers look up the converted blob via
/// `response.created.as_ref().and_then(|m| m.get(jmap_chat_client::methods::CALL_ID))`.
pub const CALL_ID: &str = "r1";

/// Capability URIs for standard JMAP Chat method calls (RFC 8620 §3.3).
pub(crate) const USING_CHAT: &[&str] =
    &["urn:ietf:params:jmap:core", jmap_chat_types::JMAP_CHAT_URI];

/// Capability URIs for Quota method calls.
pub(crate) const USING_QUOTA: &[&str] =
    &["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:quota"];

/// Capability URIs for PushSubscription method calls (RFC 8620 §7.2).
pub(crate) const USING_CORE: &[&str] = &["urn:ietf:params:jmap:core"];

/// Capability URIs for PushSubscription/set with chat push extension.
pub(crate) const USING_CHAT_PUSH: &[&str] = &[
    "urn:ietf:params:jmap:core",
    jmap_chat_types::JMAP_CHAT_PUSH_URI,
];

// ---------------------------------------------------------------------------
// build_request helper
// ---------------------------------------------------------------------------

/// Build a single-method JMAP request.
///
/// `using` is the complete `using` array for the request (RFC 8620 §3.3).
/// Use the pre-defined constants [`USING_CHAT`], [`USING_QUOTA`], or
/// [`USING_CORE`] to avoid per-call allocations.
///
/// The embedded call-id is [`CALL_ID`]; pass it directly to
/// `jmap_base_client::extract_response`.
pub(crate) fn build_request(
    method: &str,
    args: serde_json::Value,
    using: &[&str],
) -> jmap_types::JmapRequest {
    let using_vec: Vec<String> = using.iter().map(|&s| s.to_owned()).collect();
    let invocation: jmap_types::Invocation = (method.to_owned(), args, CALL_ID.to_owned());
    jmap_types::JmapRequest::new(using_vec, vec![invocation], None)
}

// ---------------------------------------------------------------------------
// resolve_client_id helper
// ---------------------------------------------------------------------------

/// Resolve an optional caller-supplied client ID, generating a ULID if absent.
///
/// Returns the supplied string unchanged, or a freshly generated ULID when
/// `None` or empty.
pub(crate) fn resolve_client_id(id: Option<&str>) -> String {
    match id {
        Some(s) if !s.is_empty() => s.to_owned(),
        _ => ulid::Ulid::new().to_string(),
    }
}

// ---------------------------------------------------------------------------
// SessionClient — session-bound client
// ---------------------------------------------------------------------------

/// A `JmapClient` bound to a JMAP session.
///
/// Obtain via the chat extension methods that accept a `Session`. All JMAP
/// Chat methods are available on this type without needing to pass `&Session`
/// on every call.
///
/// # Session lifecycle
///
/// `SessionClient` captures the `Session` at construction time. JMAP sessions
/// can expire; after re-fetching the session via `JmapClient::fetch_session`,
/// construct a new `SessionClient` with the updated session. Reusing a stale
/// `SessionClient` after session expiry will result in `unknownAccount` or
/// similar errors from the server.
///
/// `Clone` is derived because `JmapClient` is itself cheap-to-clone (it
/// already implements `Clone` and `with_chat_session` clones one
/// internally), enabling parallel-task fan-out with one bound session.
///
/// # Concurrency
///
/// `SessionClient` is `Send + Sync` and may be used from any tokio
/// task. Cloning is cheap: the underlying `JmapClient` wraps a
/// `reqwest::Client` that is itself `Clone` over an internal `Arc`,
/// so every clone shares the same HTTP connection pool. There is no
/// benefit to constructing independent `SessionClient`s for
/// per-worker fan-out — clone the existing one.
///
/// Concurrent calls on a single `SessionClient` (or on clones) are
/// safe but **unordered**: `reqwest` does not guarantee that
/// responses arrive in request order across concurrent
/// in-flight calls. When ordering matters between method calls (e.g.
/// a `Foo/set` followed by a `Foo/changes` that must observe the new
/// state), use a single JMAP batch request with `ResultReference`s
/// rather than two separate `SessionClient` calls.
///
/// All public async methods on `SessionClient` are cancel-safe at
/// each `.await` point: dropping the future before completion
/// releases the underlying `reqwest` request without poisoning the
/// connection pool. The server may still process the request (HTTP
/// is fire-and-forget on the wire); callers that care about
/// at-most-once semantics must enforce idempotency at the JMAP layer
/// (e.g. by sending a stable client-assigned creation key for
/// `/set` create operations).
///
/// `Debug` is implemented manually to redact the inner `JmapClient` (which
/// holds an HTTP client and is intentionally not `Debug` in
/// `jmap-base-client`); only the `Session` is shown. This lets callers
/// embed a `SessionClient` in a `#[derive(Debug)]` struct without manual
/// impls of their own.
///
/// # Thread safety
///
/// `SessionClient` is `Send + Sync`. Both
/// [`jmap_base_client::JmapClient`] (backed by `reqwest::Client`) and
/// [`jmap_base_client::Session`] (plain serde-derived data) are
/// `Send + Sync` per jmap-base-client's contract, so this type can be
/// shared across async tasks via `Arc<SessionClient>` or cloned for
/// per-task ownership.
///
/// A `Send + Sync` regression in a future jmap-base-client release
/// would be a major-version-breaking change for this crate. A
/// compile-time assertion in `methods/mod.rs` guards against the
/// regression landing silently — see
/// `_assert_session_client_send_sync`.
#[non_exhaustive]
#[derive(Clone)]
pub struct SessionClient {
    pub(crate) client: jmap_base_client::JmapClient,
    pub(crate) session: jmap_base_client::Session,
}

impl std::fmt::Debug for SessionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionClient")
            // The inner JmapClient is not Debug — show a placeholder so
            // callers know it is present without leaking HTTP-client
            // internals.
            .field("client", &"<JmapClient>")
            .field("session", &self.session)
            .finish()
    }
}

impl SessionClient {
    /// Borrow the underlying [`JmapClient`](jmap_base_client::JmapClient).
    ///
    /// Useful for ad-hoc operations outside the typed JMAP method surface —
    /// for example, calling `JmapClient::upload` / `JmapClient::download_blob`,
    /// or constructing a `JmapClient::event_source` subscription using the
    /// bound session's `event_source_url`.
    pub fn client(&self) -> &jmap_base_client::JmapClient {
        &self.client
    }

    /// Borrow the captured [`Session`](jmap_base_client::Session).
    ///
    /// `SessionClient` captures the `Session` at construction time. After
    /// re-fetching the session via `JmapClient::fetch_session`, callers
    /// should construct a new `SessionClient`. This accessor lets a caller
    /// compare the captured session's `state` field against a freshly
    /// fetched session to detect staleness, or inspect
    /// `accountCapabilities` / `primary_accounts` for capability-specific
    /// metadata not exposed via the typed JMAP method surface.
    pub fn session(&self) -> &jmap_base_client::Session {
        &self.session
    }

    /// Return the primary account id for `urn:ietf:params:jmap:chat`,
    /// or `Err(InvalidSession)` if the session has no primary account for
    /// that capability.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the session has no primary account for the chat capability
    ///   URI. This is the only failure mode — the method is a pure
    ///   accessor on the captured session and does no network I/O.
    pub fn chat_account_id(&self) -> Result<&str, jmap_base_client::ClientError> {
        self.session
            .primary_account_id(jmap_chat_types::JMAP_CHAT_URI)
            .ok_or_else(|| {
                jmap_base_client::ClientError::InvalidSession(
                    "no primary account for urn:ietf:params:jmap:chat".into(),
                )
            })
    }

    /// Extract `(api_url, chat_account_id)` from the bound session.
    ///
    /// Returns `Err(InvalidSession)` if there is no primary account for
    /// `urn:ietf:params:jmap:chat`.
    pub(crate) fn session_parts(&self) -> Result<(&str, &str), jmap_base_client::ClientError> {
        let api_url = self.session.api_url.as_str();
        let account_id = self
            .session
            .primary_account_id(jmap_chat_types::JMAP_CHAT_URI)
            .ok_or_else(|| {
                jmap_base_client::ClientError::InvalidSession(
                    "no primary account for urn:ietf:params:jmap:chat".into(),
                )
            })?;
        Ok((api_url, account_id))
    }

    /// The JMAP API URL from the bound session.
    pub(crate) fn api_url(&self) -> &str {
        self.session.api_url.as_str()
    }

    /// Forward a JMAP request to the underlying HTTP client.
    pub(crate) async fn call_internal(
        &self,
        api_url: &str,
        req: &jmap_types::JmapRequest,
    ) -> Result<jmap_types::JmapResponse, jmap_base_client::ClientError> {
        self.client.call(api_url, req).await
    }
}

/// Compile-time assertion that [`SessionClient`] is `Send + Sync`.
///
/// The `# Thread safety` section of [`SessionClient`]'s rustdoc promises
/// auto-trait inheritance from
/// [`jmap_base_client::JmapClient`] and
/// [`jmap_base_client::Session`]. If a future jmap-base-client release
/// adds a `!Sync` interior-mutability field to either, this assertion
/// fails at compile time — flagging the regression at the dependency
/// upgrade rather than at the downstream caller's "cannot send between
/// threads safely" error.
#[allow(dead_code)]
fn _assert_session_client_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SessionClient>();
}

// ---------------------------------------------------------------------------
// Input/patch types for methods with many optional parameters
// ---------------------------------------------------------------------------

/// Input parameters for `Chat/query`.
#[non_exhaustive]
#[derive(Debug, Default, Clone)]
pub struct ChatQueryInput {
    /// Filter to chats of the given kind (`direct`, `group`, or `channel`).
    pub filter_kind: Option<jmap_chat_types::ChatKind>,
    /// Filter to muted (`true`) or unmuted (`false`) chats.
    pub filter_muted: Option<bool>,
    /// Zero-based starting offset within the query result.
    pub position: Option<u64>,
    /// Maximum number of ids to return.
    pub limit: Option<u64>,
}

/// Input parameters for `Message/query`.
#[non_exhaustive]
#[derive(Debug, Default, Clone)]
pub struct MessageQueryInput<'a> {
    /// Restrict to messages in a specific Chat.
    pub chat_id: Option<&'a Id>,
    /// Filter to messages that mention (`true`) or do not mention (`false`) the caller.
    pub has_mention: Option<bool>,
    /// Filter to messages that carry (`true`) or do not carry (`false`) attachments.
    pub has_attachment: Option<bool>,
    /// Full-text search query against the message body.
    pub text: Option<&'a str>,
    /// Restrict to replies under this thread root.
    pub thread_root_id: Option<&'a Id>,
    /// Only include messages received after this time (exclusive).
    pub after: Option<&'a jmap_types::UTCDate>,
    /// Only include messages received before this time (exclusive).
    pub before: Option<&'a jmap_types::UTCDate>,
    /// Zero-based starting offset within the query result.
    pub position: Option<u64>,
    /// Maximum number of ids to return.
    pub limit: Option<u64>,
    /// Sort by `sentAt` ascending (oldest first) when `true`.
    /// Defaults to `false` (descending, newest first), so `position:0, limit:N`
    /// returns the N most recent messages.
    pub sort_ascending: bool,
}

impl<'a> MessageQueryInput<'a> {
    /// Set ascending sort order (oldest first).
    pub fn with_sort_ascending(mut self, v: bool) -> Self {
        self.sort_ascending = v;
        self
    }
}

/// Input parameters for `Message/set` create.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct MessageCreateInput<'a> {
    /// Caller-supplied creation key. When `None` — **or `Some("")`** — a ULID
    /// is generated automatically. An empty `Some("")` is treated the same
    /// as `None`, not as a rejection; callers must not assume the wire
    /// echoes a caller-supplied empty string.
    pub client_id: Option<&'a str>,
    /// The Chat this message belongs to.
    pub chat_id: &'a Id,
    /// Message body text (interpreted per `body_type`).
    ///
    /// Length is not checked on the client. The server enforces
    /// [`ChatCapability::max_body_bytes`](crate::ChatCapability) (UTF-8
    /// byte count for `text/*` body types, raw byte count otherwise);
    /// oversize bodies surface as a server-side `tooLarge`
    /// [`SetError`] on the `created` entry, not as a client-side
    /// rejection.
    pub body: &'a str,
    /// MIME type for the message body.
    pub body_type: jmap_chat_types::BodyType,
    /// RFC 3339 timestamp.
    ///
    /// [`UTCDate`](jmap_types::UTCDate) is a transparent newtype around
    /// `String` and does not validate the RFC 3339 format at
    /// construction time. A malformed value forwards to the server,
    /// which rejects it with `invalidArguments`. Construct values via a
    /// validating helper (e.g. `chrono::DateTime::<Utc>::to_rfc3339`)
    /// rather than from arbitrary strings.
    pub sent_at: &'a jmap_types::UTCDate,
    /// When `Some`, marks this message as a reply to the given message id.
    pub reply_to: Option<&'a Id>,
}

impl<'a> MessageCreateInput<'a> {
    /// Create a `MessageCreateInput` with required fields; optional fields default to `None`.
    pub fn new(
        chat_id: &'a Id,
        body: &'a str,
        body_type: jmap_chat_types::BodyType,
        sent_at: &'a jmap_types::UTCDate,
    ) -> Self {
        Self {
            client_id: None,
            chat_id,
            body,
            body_type,
            sent_at,
            reply_to: None,
        }
    }

    /// Set the caller-supplied creation key (overrides the auto-generated ULID).
    pub fn with_client_id(mut self, id: &'a str) -> Self {
        self.client_id = Some(id);
        self
    }

    /// Set the message this one replies to.
    pub fn with_reply_to(mut self, id: &'a Id) -> Self {
        self.reply_to = Some(id);
        self
    }
}

/// A single reaction change in a `Message/set` patch (JMAP Chat §4.5).
///
/// The patch key is `reactions/<senderReactionId>` (JSON Pointer).
/// `senderReactionId` is a caller-generated ID (e.g. ULID) that uniquely
/// identifies this reaction slot for the sending user in this message.
///
/// # Precondition: `sender_reaction_id`
///
/// On both variants, `sender_reaction_id` is embedded in a JSON Pointer
/// (RFC 6901) as the patch key `reactions/<id>`.
/// [`SessionClient::message_update`](crate::SessionClient::message_update)
/// rejects an `Add` or `Remove` whose `sender_reaction_id` is empty, or
/// contains `/` or `~`, with
/// [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument).
/// The slash/tilde restriction is unintuitive — emoji-shortcode systems
/// (e.g. `:slight_smile:/+1`) and any user-supplied string source naturally
/// produce these characters. Generate IDs from a constrained alphabet
/// (ULID, UUID v4, base64url) rather than from user input.
#[non_exhaustive]
#[derive(Debug)]
pub enum ReactionChange<'a> {
    /// Add a reaction. Patch value: `{emoji, sentAt}`.
    Add {
        /// Caller-generated id (e.g. ULID) identifying this reaction slot.
        ///
        /// **Precondition**: must be non-empty and must NOT contain `/`
        /// or `~`. See the enum-level "Precondition" section.
        sender_reaction_id: &'a str,
        /// Emoji shortcode or Unicode emoji to react with.
        emoji: &'a str,
        /// RFC 3339 timestamp when the reaction was made.
        sent_at: &'a jmap_types::UTCDate,
    },
    /// Remove a reaction. Patch value: null.
    Remove {
        /// Caller-generated id identifying the reaction slot to remove.
        ///
        /// **Precondition**: must be non-empty and must NOT contain `/`
        /// or `~`. See the enum-level "Precondition" section.
        sender_reaction_id: &'a str,
    },
}

/// Patch parameters for `Message/set` update.
///
/// All fields are optional; absent fields (i.e. `None`) are not included in
/// the patch (the server leaves them unchanged).
///
/// Use `..Default::default()` to fill in unused fields.
#[non_exhaustive]
#[derive(Debug, Default)]
pub struct MessagePatch<'a> {
    /// New message body text (author-only edit).
    pub body: Option<&'a str>,
    /// MIME type for `body`. Set alongside `body` in author-only edits.
    pub body_type: Option<jmap_chat_types::BodyType>,
    /// Reaction changes to apply. `None` (default) = no reaction changes.
    pub reaction_changes: Option<&'a [ReactionChange<'a>]>,
    /// Set the read-receipt timestamp (`Message.readAt`).
    pub read_at: Option<&'a jmap_types::UTCDate>,
    /// Set the read disposition recorded alongside `read_at`
    /// (draft-atwood-jmap-chat-00 §Message/set update, line 1012).
    ///
    /// Setting `read_at` without `read_disposition` causes the server to
    /// store `"displayed"` (§Message line 540). Supplying both lets the
    /// client pick `Deleted` or `Processed` explicitly, or use
    /// `Other("...")` for a vendor / future disposition value. Clients
    /// SHOULD only set this when they also set `read_at`.
    pub read_disposition: Option<jmap_chat_types::ReadDisposition>,
    /// Set the deletion timestamp for soft/hard delete.
    pub deleted_at: Option<&'a jmap_types::UTCDate>,
    /// When `Some(true)` and `deleted_at` is also set, deletes for all
    /// participants (server sends `Peer/retract`).
    pub deleted_for_all: Option<bool>,
}

/// Patch parameters for `PresenceStatus/set` update.
///
/// All fields are optional. A field that is `Patch::Keep` (default) is omitted
/// from the patch, leaving the server value unchanged. Use `Patch::Set(v)` to
/// set a value and `Patch::Clear` to null-clear a nullable field.
///
/// Use `..Default::default()` to fill in unused fields.
#[non_exhaustive]
#[derive(Debug, Default)]
pub struct PresenceStatusPatch<'a> {
    /// New presence state. `None` = no change.
    pub presence: Option<jmap_chat_types::Presence>,
    /// Free-text status message. [`Patch::Clear`] clears; [`Patch::Set`] sets.
    pub status_text: Patch<&'a str>,
    /// Status emoji. [`Patch::Clear`] clears; [`Patch::Set`] sets.
    pub status_emoji: Patch<&'a str>,
    /// Set or clear the auto-clear deadline. `Patch::Clear` removes any deadline.
    pub expires_at: Patch<&'a jmap_types::UTCDate>,
    /// Whether read receipts are shared with peers. `None` = no change.
    pub receipt_sharing: Option<bool>,
}

/// Input parameters for `CustomEmoji/query`.
#[non_exhaustive]
#[derive(Debug, Default, Clone)]
pub struct CustomEmojiQueryInput<'a> {
    /// Filter to a specific Space's custom emojis. `None` returns all emojis
    /// visible to the account (Space-specific + server-global).
    pub filter_space_id: Option<&'a Id>,
    /// Zero-based starting offset within the query result.
    pub position: Option<u64>,
    /// Maximum number of ids to return.
    pub limit: Option<u64>,
}

/// Parameters for creating one CustomEmoji via `CustomEmoji/set`.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct CustomEmojiCreateInput<'a> {
    /// Caller-supplied creation key. When `None` — **or `Some("")`** — a ULID
    /// is generated automatically. An empty `Some("")` is treated the same
    /// as `None`, not as a rejection; callers must not assume the wire
    /// echoes a caller-supplied empty string.
    pub client_id: Option<&'a str>,
    /// Shortcode name without colons (e.g., `catjam`).
    pub name: &'a str,
    /// blobId of the emoji image (already uploaded).
    pub blob_id: &'a Id,
    /// If `Some`, limits the emoji to the given Space. `None` = server-global.
    pub space_id: Option<&'a Id>,
}

impl<'a> CustomEmojiCreateInput<'a> {
    /// Create a `CustomEmojiCreateInput` with required fields; optional fields default to `None`.
    pub fn new(name: &'a str, blob_id: &'a Id) -> Self {
        Self {
            client_id: None,
            name,
            blob_id,
            space_id: None,
        }
    }

    /// Set the caller-supplied creation key (overrides the auto-generated ULID).
    pub fn with_client_id(mut self, id: &'a str) -> Self {
        self.client_id = Some(id);
        self
    }
}

/// Parameters for creating one SpaceInvite via `SpaceInvite/set`.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct SpaceInviteCreateInput<'a> {
    /// Caller-supplied creation key. When `None` — **or `Some("")`** — a ULID
    /// is generated automatically. An empty `Some("")` is treated the same
    /// as `None`, not as a rejection; callers must not assume the wire
    /// echoes a caller-supplied empty string.
    pub client_id: Option<&'a str>,
    /// The Space this invite grants access to.
    pub space_id: &'a Id,
    /// Channel that joining members land in by default. `None` lets the server choose.
    pub default_channel_id: Option<&'a Id>,
    /// Optional expiry time after which the invite is no longer redeemable.
    pub expires_at: Option<&'a jmap_types::UTCDate>,
    /// Maximum number of times the invite may be redeemed.
    pub max_uses: Option<u64>,
}

impl<'a> SpaceInviteCreateInput<'a> {
    /// Create a `SpaceInviteCreateInput` with required fields; optional fields default to `None`.
    pub fn new(space_id: &'a Id) -> Self {
        Self {
            client_id: None,
            space_id,
            default_channel_id: None,
            expires_at: None,
            max_uses: None,
        }
    }

    /// Set the caller-supplied creation key (overrides the auto-generated ULID).
    pub fn with_client_id(mut self, id: &'a str) -> Self {
        self.client_id = Some(id);
        self
    }

    /// Set the maximum number of times this invite may be used.
    pub fn with_max_uses(mut self, max: u64) -> Self {
        self.max_uses = Some(max);
        self
    }
}

/// Parameters for creating one SpaceBan via `SpaceBan/set`.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct SpaceBanCreateInput<'a> {
    /// Caller-supplied creation key. When `None` — **or `Some("")`** — a ULID
    /// is generated automatically. An empty `Some("")` is treated the same
    /// as `None`, not as a rejection; callers must not assume the wire
    /// echoes a caller-supplied empty string.
    pub client_id: Option<&'a str>,
    /// The Space this ban applies to.
    pub space_id: &'a Id,
    /// ChatContact.id of the user to ban.
    pub user_id: &'a Id,
    /// Optional human-readable reason for the ban.
    pub reason: Option<&'a str>,
    /// Optional expiry time after which the ban is automatically lifted.
    pub expires_at: Option<&'a jmap_types::UTCDate>,
}

impl<'a> SpaceBanCreateInput<'a> {
    /// Create a `SpaceBanCreateInput` with required fields; optional fields default to `None`.
    pub fn new(space_id: &'a Id, user_id: &'a Id) -> Self {
        Self {
            client_id: None,
            space_id,
            user_id,
            reason: None,
            expires_at: None,
        }
    }

    /// Set the caller-supplied creation key (overrides the auto-generated ULID).
    pub fn with_client_id(mut self, id: &'a str) -> Self {
        self.client_id = Some(id);
        self
    }
}

/// Patch parameters for `ChatContact/set` update.
///
/// All fields are optional; absent fields are omitted from the patch. For the
/// nullable `display_name` field, use `Patch::Set(s)` to set and `Patch::Clear`
/// to clear. Use `..Default::default()` to fill in unused fields.
#[non_exhaustive]
#[derive(Debug, Default)]
pub struct ChatContactPatch<'a> {
    /// Set or unset the blocked flag on this contact. `None` = no change.
    pub blocked: Option<bool>,
    /// `Patch::Clear` clears `displayName`; `Patch::Set(s)` sets it.
    pub display_name: Patch<&'a str>,
}

/// Sort property for `ChatContact/query`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ContactSortProperty {
    /// Sort by the contact's `lastSeenAt` timestamp.
    LastSeenAt,
    /// Sort by the contact's `login` identifier.
    Login,
    /// Sort by the contact's `lastActiveAt` timestamp.
    LastActiveAt,
}

/// Input parameters for `ChatContact/query`.
///
/// All fields are optional; an empty filter shows all contacts.
#[non_exhaustive]
#[derive(Debug, Default, Clone)]
pub struct ChatContactQueryInput {
    /// Filter to blocked (`true`) or non-blocked (`false`) contacts.
    pub filter_blocked: Option<bool>,
    /// Filter to contacts with this exact presence state.
    pub filter_presence: Option<crate::types::ContactPresenceFilter>,
    /// Zero-based starting offset within the query result.
    pub position: Option<u64>,
    /// Maximum number of ids to return.
    pub limit: Option<u64>,
    /// Sort property.
    pub sort_property: Option<ContactSortProperty>,
    /// When `Some(false)` or `None`, sort descending. `Some(true)` sorts ascending.
    pub sort_ascending: Option<bool>,
}

/// Input parameters for `Space/set` create.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct SpaceCreateInput<'a> {
    /// Caller-supplied creation key. When `None` — **or `Some("")`** — a ULID
    /// is generated automatically. An empty `Some("")` is treated the same
    /// as `None`, not as a rejection; callers must not assume the wire
    /// echoes a caller-supplied empty string.
    pub client_id: Option<&'a str>,
    /// Display name for the Space.
    pub name: &'a str,
    /// Optional human-readable description.
    pub description: Option<&'a str>,
    /// Optional blob id of an already-uploaded icon image.
    pub icon_blob_id: Option<&'a Id>,
}

impl<'a> SpaceCreateInput<'a> {
    /// Create a `SpaceCreateInput` with required fields; optional fields default to `None`.
    pub fn new(name: &'a str) -> Self {
        Self {
            client_id: None,
            name,
            description: None,
            icon_blob_id: None,
        }
    }

    /// Set the caller-supplied creation key (overrides the auto-generated ULID).
    pub fn with_client_id(mut self, id: &'a str) -> Self {
        self.client_id = Some(id);
        self
    }
}

/// Input parameters for `Space/query`.
#[non_exhaustive]
#[derive(Debug, Default, Clone)]
pub struct SpaceQueryInput<'a> {
    /// Filter by substring match on Space name.
    pub filter_name: Option<&'a str>,
    /// Filter to public (`true`) or non-public (`false`) Spaces.
    pub filter_is_public: Option<bool>,
    /// Zero-based starting offset within the query result.
    pub position: Option<u64>,
    /// Maximum number of ids to return.
    pub limit: Option<u64>,
}

/// How to join a Space — passed to `Space/join`.
///
/// The enum makes invalid inputs unrepresentable: exactly one path is always
/// selected at construction time.
///
/// # Debug redaction
///
/// The `InviteCode` variant wraps the unguessable bearer credential from
/// draft-atwood-jmap-chat-00 §4.18 — anyone with the code can redeem it to
/// join the Space. The `Debug` impl on this enum redacts the inner string to
/// `"[REDACTED]"` so an accidental `{:?}`-format in an application log,
/// tracing span, or test fixture cannot leak it. The `SpaceId` variant is
/// not a secret (RFC 8620 §1.2) and is rendered verbatim.
#[non_exhaustive]
pub enum SpaceJoinInput<'a> {
    /// Redeem a SpaceInvite by its `code` field (not its `id`).
    ///
    /// Unguessable secret — redacted by the [`std::fmt::Debug`] impl on this enum.
    ///
    /// **Precondition**: must be non-empty.
    /// [`Self::space_join`](crate::SessionClient::space_join) rejects
    /// an empty value client-side with
    /// [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument).
    InviteCode(&'a str),
    /// Join a public Space directly by its JMAP id.
    ///
    /// **Precondition**: must be a non-empty Id. [`Id::from`] is the
    /// lenient constructor and accepts any string including `""`;
    /// `Id::new_validated` is the strict one. An empty Id forwards to
    /// the server unchanged, which rejects it with `invalidArguments`.
    /// Construct Ids via the validating path when the value comes from
    /// untrusted input.
    SpaceId(&'a Id),
}

impl<'a> std::fmt::Debug for SpaceJoinInput<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InviteCode(_) => f.debug_tuple("InviteCode").field(&"[REDACTED]").finish(),
            Self::SpaceId(id) => f.debug_tuple("SpaceId").field(id).finish(),
        }
    }
}

/// One entry in the `addMembers` patch key for `Chat/set` update.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct AddMemberInput<'a> {
    /// ChatContact.id of the member to add.
    pub id: &'a Id,
    /// Role for the new member. `None` lets the server apply the default (`"member"`).
    pub role: Option<crate::types::ChatMemberRole>,
}

impl<'a> AddMemberInput<'a> {
    /// Create an `AddMemberInput`; `role` defaults to `None` (server assigns default).
    pub fn new(id: &'a Id) -> Self {
        Self { id, role: None }
    }

    /// Set the role for this member.
    pub fn with_role(mut self, role: crate::types::ChatMemberRole) -> Self {
        self.role = Some(role);
        self
    }
}

/// One entry in the `updateMemberRoles` patch key for `Chat/set` update.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct UpdateMemberRoleInput<'a> {
    /// ChatContact.id of the member to update.
    pub id: &'a Id,
    /// New role for this member.
    pub role: crate::types::ChatMemberRole,
}

impl<'a> UpdateMemberRoleInput<'a> {
    /// Create an `UpdateMemberRoleInput` with the target member and their new role.
    pub fn new(id: &'a Id, role: crate::types::ChatMemberRole) -> Self {
        Self { id, role }
    }
}

/// Input parameters for `Chat/set` create.
///
/// Discriminates the two user-creatable Chat kinds from the spec. Each
/// variant carries the fields required for that kind plus an optional
/// `client_id`; when `None` — or `Some("")` — a ULID is generated
/// automatically (see the per-variant `client_id` doc).
///
/// Channel Chats are NOT created via `Chat/set`. Per
/// draft-atwood-jmap-chat-00 §Chat (line 436), Channel Chats are created
/// as part of a Space via the `addChannels` patch key in `Space/set`
/// (see [`SpacePatch::add_channels`] and [`SpaceAddChannelInput`]); the
/// server assigns the channel's chatId at that time. A spec-compliant
/// server will reject a `Chat/set` create with `kind: "channel"`.
#[non_exhaustive]
#[derive(Debug)]
pub enum ChatCreateInput<'a> {
    /// Create a direct (one-to-one) chat.
    Direct {
        /// Caller-supplied creation key. When `None` — **or `Some("")`** —
        /// a ULID is generated automatically. An empty `Some("")` is
        /// treated the same as `None`, not as a rejection.
        client_id: Option<&'a str>,
        /// ChatContact.id of the other participant.
        contact_id: &'a Id,
    },
    /// Create a group chat.
    Group {
        /// Caller-supplied creation key. When `None` — **or `Some("")`** —
        /// a ULID is generated automatically. An empty `Some("")` is
        /// treated the same as `None`, not as a rejection.
        client_id: Option<&'a str>,
        /// Display name for the group.
        name: &'a str,
        /// ChatContact.ids of initial non-owner members.
        member_ids: &'a [Id],
        /// Optional human-readable description.
        description: Option<&'a str>,
        /// Blob id of an already-uploaded avatar image, if any.
        avatar_blob_id: Option<&'a Id>,
        /// Optional auto-expiry interval applied to new messages.
        message_expiry_seconds: Option<u64>,
    },
}

/// Patch parameters for `Chat/set` update.
///
/// All fields are optional; absent fields are not included in the patch (the
/// server leaves them unchanged). For nullable spec fields (`mute_until`,
/// `description`, `avatar_blob_id`) use `Patch::Set(v)` to set and
/// `Patch::Clear` to null-clear. Slice fields default to `None` (no change).
///
/// Use `..Default::default()` to fill in unused fields.
#[non_exhaustive]
#[derive(Debug, Default)]
pub struct ChatPatch<'a> {
    /// Mute or unmute this chat. `None` = no change.
    pub muted: Option<bool>,
    /// `Patch::Clear` clears `muteUntil`; `Patch::Set(t)` sets it.
    pub mute_until: Patch<&'a jmap_types::UTCDate>,
    /// Whether typing indicators from peers are surfaced to the caller. `None` = no change.
    pub receive_typing_indicators: Option<bool>,
    /// Replace the entire pinned-message list. `Some(&[])` clears all pins.
    pub pinned_message_ids: Option<&'a [Id]>,
    /// Local message-expiry policy in seconds (draft-atwood-jmap-chat-00
    /// §Chat line 505: `messageExpirySeconds (UnsignedInt, optional)`).
    /// `Patch::Clear` removes the policy server-side; `Patch::Set(n)` sets
    /// it; `Patch::Keep` (default) leaves it unchanged.
    pub message_expiry_seconds: Patch<u64>,
    /// Whether read receipts are shared with peers. `None` = no change.
    pub receipt_sharing: Option<bool>,
    /// New display name (group chats, admin only).
    pub name: Option<&'a str>,
    /// `Patch::Clear` clears; `Patch::Set(s)` sets (group chats, admin only).
    pub description: Patch<&'a str>,
    /// `Patch::Clear` clears; `Patch::Set(id)` sets (group chats, admin only).
    pub avatar_blob_id: Patch<&'a Id>,
    /// Members to add (group chats, admin only). `None` = no change.
    pub add_members: Option<&'a [AddMemberInput<'a>]>,
    /// ChatContact.ids to remove (group chats, admin only). `None` = no change.
    pub remove_members: Option<&'a [Id]>,
    /// Role changes for existing members (group chats, admin only). `None` = no change.
    pub update_member_roles: Option<&'a [UpdateMemberRoleInput<'a>]>,
}

/// One member to add in the `addMembers` patch key of `Space/set` update.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct SpaceAddMemberInput<'a> {
    /// ChatContact.id of the member to add.
    pub id: &'a Id,
    /// Initial role IDs for the new member. `None` grants no extra roles beyond `@everyone`.
    pub role_ids: Option<&'a [Id]>,
}

impl<'a> SpaceAddMemberInput<'a> {
    /// Create a `SpaceAddMemberInput`; `role_ids` defaults to `None`.
    pub fn new(id: &'a Id) -> Self {
        Self { id, role_ids: None }
    }
}

/// One member update in the `updateMembers` patch key of `Space/set` update.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct SpaceUpdateMemberInput<'a> {
    /// ChatContact.id of the member to update.
    pub id: &'a Id,
    /// Replace the member's SpaceRole.id list. `None` = no change.
    pub role_ids: Option<&'a [Id]>,
    /// `Patch::Clear` clears the nick; `Patch::Set(s)` sets it.
    pub nick: Patch<&'a str>,
}

impl<'a> SpaceUpdateMemberInput<'a> {
    /// Create a `SpaceUpdateMemberInput`; optional fields default to `None`/`Keep`.
    pub fn new(id: &'a Id) -> Self {
        Self {
            id,
            role_ids: None,
            nick: Patch::Keep,
        }
    }
}

/// One channel to add in the `addChannels` patch key of `Space/set` update.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct SpaceAddChannelInput<'a> {
    /// Channel display name.
    pub name: &'a str,
    /// Optional parent category id. `None` places the channel in `uncategorizedChannelIds`.
    pub category_id: Option<&'a Id>,
    /// Optional position within the category.
    pub position: Option<u64>,
    /// Optional channel topic.
    pub topic: Option<&'a str>,
}

impl<'a> SpaceAddChannelInput<'a> {
    /// Create a `SpaceAddChannelInput`; optional fields default to `None`.
    pub fn new(name: &'a str) -> Self {
        Self {
            name,
            category_id: None,
            position: None,
            topic: None,
        }
    }
}

/// One role to add in the `addRoles` patch key of `Space/set` update
/// (JMAP Chat §Space/set update / `manage_roles` permission).
///
/// The server assigns the role's ULID; the request never specifies an `id`.
/// Hierarchy enforcement: a member may only add roles whose `position` is
/// strictly less than their own highest-position role (server-enforced).
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct SpaceAddRoleInput<'a> {
    /// Human-readable role name.
    pub name: &'a str,
    /// Permission identifier strings, e.g. `"manage_channels"`.
    pub permissions: &'a [&'a str],
    /// Position in the role hierarchy. Lower values sort first.
    pub position: u64,
    /// Optional CSS-style color string (e.g. `"#ff8800"`). Pass `None` to omit.
    pub color: Option<&'a str>,
}

impl<'a> SpaceAddRoleInput<'a> {
    /// Create a `SpaceAddRoleInput` with required fields; optional `color` defaults to `None`.
    pub fn new(name: &'a str, permissions: &'a [&'a str], position: u64) -> Self {
        Self {
            name,
            permissions,
            position,
            color: None,
        }
    }

    /// Attach a color to the role.
    pub fn with_color(mut self, color: &'a str) -> Self {
        self.color = Some(color);
        self
    }
}

/// One role update in the `updateRoles` patch key of `Space/set` update
/// (JMAP Chat §Space/set update / `manage_roles` permission).
///
/// Fields left at their default (`None` / [`Patch::Keep`]) are omitted from
/// the wire patch and the server leaves the corresponding property unchanged.
/// Hierarchy enforcement: a member may only modify roles whose `position` is
/// strictly less than their own highest-position role (server-enforced).
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct SpaceUpdateRoleInput<'a> {
    /// SpaceRole.id to update.
    pub id: &'a Id,
    /// New name. `None` = no change.
    pub name: Option<&'a str>,
    /// Set or clear the color. [`Patch::Clear`] removes any assigned color.
    pub color: Patch<&'a str>,
    /// Replace the permissions list. `None` = no change.
    pub permissions: Option<&'a [&'a str]>,
    /// New position in the role hierarchy. `None` = no change.
    pub position: Option<u64>,
}

impl<'a> SpaceUpdateRoleInput<'a> {
    /// Create a `SpaceUpdateRoleInput`; optional fields default to `None`/`Keep`.
    pub fn new(id: &'a Id) -> Self {
        Self {
            id,
            name: None,
            color: Patch::Keep,
            permissions: None,
            position: None,
        }
    }
}

/// One channel update in the `updateChannels` patch key of `Space/set` update
/// (JMAP Chat §Space/set update / `manage_channels` permission).
///
/// Fields left at their default (`None` / [`Patch::Keep`]) are omitted from
/// the wire patch and the server leaves the corresponding property unchanged.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct SpaceUpdateChannelInput<'a> {
    /// Channel Chat id (kind `"channel"`, `spaceId` is this Space).
    pub id: &'a Id,
    /// New channel name. `None` = no change.
    pub name: Option<&'a str>,
    /// Set or clear the channel topic. [`Patch::Clear`] removes any assigned topic.
    pub topic: Patch<&'a str>,
    /// Set or clear the parent category. [`Patch::Clear`] moves the channel to
    /// the `uncategorizedChannelIds` list.
    pub category_id: Patch<&'a Id>,
    /// New position within its category. `None` = no change.
    pub position: Option<u64>,
    /// New slow-mode delay in seconds (`0` = disabled). `None` = no change.
    pub slow_mode_seconds: Option<u64>,
    /// Replace the permission-overrides list. `None` = no change.
    pub permission_overrides: Option<&'a [jmap_chat_types::ChannelPermission]>,
}

impl<'a> SpaceUpdateChannelInput<'a> {
    /// Create a `SpaceUpdateChannelInput`; optional fields default to `None`/`Keep`.
    pub fn new(id: &'a Id) -> Self {
        Self {
            id,
            name: None,
            topic: Patch::Keep,
            category_id: Patch::Keep,
            position: None,
            slow_mode_seconds: None,
            permission_overrides: None,
        }
    }
}

/// One category to add in the `addCategories` patch key of `Space/set` update
/// (JMAP Chat §Space/set update / `manage_channels` permission).
///
/// The server assigns the category's ULID; the request never specifies an `id`.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct SpaceAddCategoryInput<'a> {
    /// Category display name.
    pub name: &'a str,
    /// Position relative to other categories. `None` lets the server append.
    pub position: Option<u64>,
    /// Initial member channel ids. `None` = empty category.
    pub channel_ids: Option<&'a [Id]>,
}

impl<'a> SpaceAddCategoryInput<'a> {
    /// Create a `SpaceAddCategoryInput`; optional fields default to `None`.
    pub fn new(name: &'a str) -> Self {
        Self {
            name,
            position: None,
            channel_ids: None,
        }
    }
}

/// One category update in the `updateCategories` patch key of `Space/set` update
/// (JMAP Chat §Space/set update / `manage_channels` permission).
///
/// Fields left at their default (`None`) are omitted from the wire patch and
/// the server leaves the corresponding property unchanged.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct SpaceUpdateCategoryInput<'a> {
    /// Category id to update.
    pub id: &'a Id,
    /// New name. `None` = no change.
    pub name: Option<&'a str>,
    /// New position. `None` = no change.
    pub position: Option<u64>,
    /// Replace the member channel id list. `None` = no change.
    pub channel_ids: Option<&'a [Id]>,
}

impl<'a> SpaceUpdateCategoryInput<'a> {
    /// Create a `SpaceUpdateCategoryInput`; optional fields default to `None`.
    pub fn new(id: &'a Id) -> Self {
        Self {
            id,
            name: None,
            position: None,
            channel_ids: None,
        }
    }
}

/// Patch parameters for `Space/set` update.
///
/// All fields are optional. Absent fields are omitted from the patch.
/// Nullable fields (`description`, `icon_blob_id`) use `Patch::Set(v)` to set
/// and `Patch::Clear` to null-clear. Slice fields default to `None` (no change).
///
/// Use `..Default::default()` to fill in unused fields.
#[non_exhaustive]
#[derive(Debug, Default)]
pub struct SpacePatch<'a> {
    /// New display name (`manage_space` permission required).
    pub name: Option<&'a str>,
    /// `Patch::Clear` clears; `Patch::Set(s)` sets.
    pub description: Patch<&'a str>,
    /// `Patch::Clear` clears; `Patch::Set(id)` sets.
    pub icon_blob_id: Patch<&'a Id>,
    /// Toggle public-Space visibility. `None` = no change.
    pub is_public: Option<bool>,
    /// Toggle whether a public Space is previewable to non-members. `None` = no change.
    pub is_publicly_previewable: Option<bool>,
    /// Members to add (`manage_members` required). `None` = no change.
    pub add_members: Option<&'a [SpaceAddMemberInput<'a>]>,
    /// ChatContact.ids to remove (`manage_members` required). `None` = no change.
    pub remove_members: Option<&'a [Id]>,
    /// Member updates (`manage_members` required). `None` = no change.
    pub update_members: Option<&'a [SpaceUpdateMemberInput<'a>]>,
    /// Channels to add (`manage_channels` required). `None` = no change.
    pub add_channels: Option<&'a [SpaceAddChannelInput<'a>]>,
    /// Channel Chat ids to remove (`manage_channels` required). `None` = no change.
    pub remove_channels: Option<&'a [Id]>,
    /// Channel updates (`manage_channels` required). `None` = no change.
    pub update_channels: Option<&'a [SpaceUpdateChannelInput<'a>]>,
    /// Roles to add (`manage_roles` required). `None` = no change.
    pub add_roles: Option<&'a [SpaceAddRoleInput<'a>]>,
    /// SpaceRole.ids to remove (`manage_roles` required). `None` = no change.
    pub remove_roles: Option<&'a [Id]>,
    /// Role updates (`manage_roles` required). `None` = no change.
    pub update_roles: Option<&'a [SpaceUpdateRoleInput<'a>]>,
    /// Categories to add (`manage_channels` required). `None` = no change.
    pub add_categories: Option<&'a [SpaceAddCategoryInput<'a>]>,
    /// Category ids to remove (`manage_channels` required). `None` = no change.
    pub remove_categories: Option<&'a [Id]>,
    /// Category updates (`manage_channels` required). `None` = no change.
    pub update_categories: Option<&'a [SpaceUpdateCategoryInput<'a>]>,
}

/// Input parameters for `PushSubscription/set` create (RFC 8620 §7.2).
///
/// Creates a PushSubscription with the optional `chatPush` extension
/// (draft-atwood-jmap-chat-push-00 §3.1).
///
/// `device_client_id` and `url` have no safe defaults and must always be supplied.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct PushSubscriptionCreateInput<'a> {
    /// Caller-supplied creation key. When `None` — **or `Some("")`** — a ULID
    /// is generated automatically. An empty `Some("")` is treated the same
    /// as `None`, not as a rejection; callers must not assume the wire
    /// echoes a caller-supplied empty string.
    pub client_id: Option<&'a str>,
    /// Stable client device identifier, used by the server to deduplicate subscriptions.
    ///
    /// **Precondition**: must be non-empty.
    /// [`Self::push_subscription_create`](crate::SessionClient::push_subscription_create)
    /// rejects an empty value client-side with
    /// [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument).
    pub device_client_id: &'a str,
    /// Push endpoint URL registered with the platform push service.
    ///
    /// **Precondition**: must be non-empty.
    /// [`Self::push_subscription_create`](crate::SessionClient::push_subscription_create)
    /// rejects an empty value client-side with
    /// [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument).
    /// Malformed-but-non-empty URLs forward to the server, which rejects
    /// them with a `setError` on the creation entry. No client-side URL
    /// syntax check is performed.
    pub url: &'a str,
    /// Subscription expiry time. `None` lets the server choose.
    pub expires: Option<&'a jmap_types::UTCDate>,
    /// Data type names to include in StateChange notifications.
    /// `None` means the server delivers all changed types.
    pub types: Option<&'a [&'a str]>,
    /// Per-account ChatPushConfig entries for inline push. Each entry is
    /// `(accountId, config)`. Pass `None` to omit the `chatPush` property.
    ///
    /// # Why a slice-of-pairs, not a `HashMap`?
    ///
    /// The wire form is a JSON object keyed by accountId (a `HashMap`
    /// shape). The slice-of-pairs input form is deliberate:
    ///
    /// * Most callers know their per-account configs at construction
    ///   time and can supply them as an array literal; forcing
    ///   `&HashMap<Id, _>` would require a `.collect()` or a
    ///   `HashMap::from([...])` site at every call.
    /// * [`Id`] does not derive [`Ord`], so a
    ///   deterministic-iteration alternative would have to be
    ///   `BTreeMap<Id, _>`, which requires a workspace-wide Ord
    ///   propagation pass to add. `HashMap<Id, _>` would give
    ///   non-deterministic JSON output (test fixtures and snapshot
    ///   tests would become order-flaky).
    /// * The runtime duplicate-accountId check
    ///   ([`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument))
    ///   is small, shared between create and update via
    ///   `build_chat_push_map`, and has unit-test coverage.
    ///
    /// The trade-off is one runtime check per call site against an
    /// ergonomic gain for the common case. If a future workspace pass
    /// adds [`Ord`] to [`Id`], a follow-up bead can re-evaluate.
    pub chat_push: Option<&'a [(&'a Id, jmap_chat_types::ChatPushConfig)]>,
}

impl<'a> PushSubscriptionCreateInput<'a> {
    /// Create a `PushSubscriptionCreateInput` with required fields; optional fields default to `None`.
    pub fn new(device_client_id: &'a str, url: &'a str) -> Self {
        Self {
            client_id: None,
            device_client_id,
            url,
            expires: None,
            types: None,
            chat_push: None,
        }
    }

    /// Set the caller-supplied creation key (overrides the auto-generated ULID).
    pub fn with_client_id(mut self, id: &'a str) -> Self {
        self.client_id = Some(id);
        self
    }

    /// Restrict StateChange notifications to these data type names.
    pub fn with_types(mut self, types: &'a [&'a str]) -> Self {
        self.types = Some(types);
        self
    }

    /// Attach per-account ChatPushConfig entries for inline push.
    pub fn with_chat_push(
        mut self,
        chat_push: &'a [(&'a Id, jmap_chat_types::ChatPushConfig)],
    ) -> Self {
        self.chat_push = Some(chat_push);
        self
    }
}

/// Patch shape for `PushSubscription/set` update sub-operations (RFC 8620 §7.2.2).
///
/// Only the patchable properties are exposed. RFC 8620 §7.2 declares `url`
/// and `keys` immutable: to change those, destroy the subscription and create
/// a new one. `device_client_id` is also stable for the lifetime of the
/// subscription.
///
/// Fields left as their default ([`Patch::Keep`] / `None` for
/// `verification_code`) are omitted from the wire patch and the server leaves
/// the corresponding property unchanged.
///
/// The `types` and `chat_push` fields use the three-way [`Patch<T>`] shape
/// so the (set + clear) conflict is unrepresentable at the type level:
///   - [`Patch::Set`] replaces the property wholesale (an array of type
///     names for `types`; a `chatPush` object built from the supplied
///     `(accountId, config)` pairs for `chat_push`).
///   - [`Patch::Clear`] sends JSON `null`, removing the property
///     server-side. For `types`, `null` means "deliver all types"
///     (RFC 8620 §7.2). For `chat_push`, `null` removes all inline-push
///     configuration (draft-atwood-jmap-chat-push-00 §3.1).
///   - [`Patch::Keep`] (default) omits the key from the patch.
///
/// The JMAP Chat Push extension does not define per-key patching of
/// `chatPush`, so [`Patch::Set`] replaces the full map.
///
/// # Debug redaction
///
/// `verification_code` is the RFC 8620 §7.2 push-subscription-ownership
/// proof — an attacker who learns the value can claim ownership of the
/// subscription. The `Debug` impl on this struct redacts it to
/// `Some("[REDACTED]")` / `None` so an accidental `{:?}`-format in an
/// application log, tracing span, or test fixture cannot leak it. Other
/// fields are not secrets and are rendered verbatim.
#[non_exhaustive]
#[derive(Default)]
pub struct PushSubscriptionPatch<'a> {
    /// Replace the verification code (set after receiving a PushVerification
    /// payload). `None` = no change.
    ///
    /// RFC 8620 §7.2 ownership proof — redacted by the
    /// [`std::fmt::Debug`] impl on this struct.
    pub verification_code: Option<&'a str>,
    /// Set or clear the expiry timestamp. [`Patch::Clear`] sets `expires` to
    /// `null`; the server SHOULD then choose a default expiry per RFC 8620 §7.2.
    pub expires: Patch<&'a jmap_types::UTCDate>,
    /// Replace or clear the `types` filter (RFC 8620 §7.2). [`Patch::Set`]
    /// installs the array of type names; [`Patch::Clear`] sends JSON `null`
    /// so the server delivers all types; [`Patch::Keep`] (default) leaves
    /// the property unchanged.
    pub types: Patch<&'a [&'a str]>,
    /// Replace or clear the `chatPush` extension property
    /// (draft-atwood-jmap-chat-push-00 §3.1). [`Patch::Set`] installs the
    /// `chatPush` map from the supplied `(accountId, config)` pairs;
    /// [`Patch::Clear`] sends JSON `null` so the server removes all inline
    /// push config; [`Patch::Keep`] (default) leaves the property
    /// unchanged.
    ///
    /// See the rustdoc on
    /// [`PushSubscriptionCreateInput::chat_push`] for the rationale
    /// behind the slice-of-pairs input shape (vs `HashMap<Id, _>`).
    pub chat_push: Patch<&'a [(&'a Id, jmap_chat_types::ChatPushConfig)]>,
}

impl<'a> std::fmt::Debug for PushSubscriptionPatch<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let redacted_verification_code: Option<&'static str> =
            self.verification_code.map(|_| "[REDACTED]");
        f.debug_struct("PushSubscriptionPatch")
            .field("verification_code", &redacted_verification_code)
            .field("expires", &self.expires)
            .field("types", &self.types)
            .field("chat_push", &self.chat_push)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Method sub-modules
// ---------------------------------------------------------------------------

pub mod chat;
pub mod message;
pub mod space;

// ---------------------------------------------------------------------------
// SetError extension accessors (JMAP Chat slow-mode)
// ---------------------------------------------------------------------------

/// Error from [`server_retry_after`] when the `serverRetryAfter` field is
/// present but doesn't parse as a [`jmap_types::UTCDate`].
///
/// The raw JSON value is preserved for diagnostics — callers can log it
/// to surface server-side bugs (e.g. emitting a numeric-seconds form
/// when the workspace convention is a UTCDate string).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerRetryAfterError {
    /// The `serverRetryAfter` field was present in the `SetError.extra`
    /// map but its value did not deserialise into a `UTCDate`. The raw
    /// JSON value is included for diagnostics.
    Malformed(serde_json::Value),
}

impl std::fmt::Display for ServerRetryAfterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServerRetryAfterError::Malformed(raw) => {
                write!(f, "serverRetryAfter present but malformed: {raw}")
            }
        }
    }
}

impl std::error::Error for ServerRetryAfterError {}

/// Read the `serverRetryAfter` extension field from a [`SetError`] per the
/// JMAP Chat slow-mode workspace convention.
///
/// The base [`jmap_types::SetError`] type captures unknown extension
/// fields in its `extra` map via `#[serde(flatten)]`. JMAP Chat's
/// `rateLimited` error includes a `serverRetryAfter` UTCDate telling
/// the client when it may retry. This accessor distinguishes the three
/// possible outcomes so callers can react appropriately:
///
/// - `Ok(None)` — the field is absent. The server is using `rateLimited`
///   without a retry hint; the caller may apply a default backoff.
/// - `Ok(Some(t))` — the field is present and parsed; honour `t`.
/// - `Err(ServerRetryAfterError::Malformed(raw))` — the field is
///   present but its value is not a valid `UTCDate`. The raw JSON is
///   returned so the caller can log a server-side bug; the caller
///   should still apply a default backoff (the bug is server-side).
///
/// Prior to bd:JMAP-26di.50 this function returned a single `Option`
/// that conflated absent and malformed; the new shape gives the caller
/// the diagnostic information it needs to distinguish them.
pub fn server_retry_after(
    err: &SetError,
) -> Result<Option<jmap_types::UTCDate>, ServerRetryAfterError> {
    match err.extra.get("serverRetryAfter") {
        None => Ok(None),
        Some(v) => match jmap_types::UTCDate::deserialize(v) {
            Ok(d) => Ok(Some(d)),
            Err(_) => Err(ServerRetryAfterError::Malformed(v.clone())),
        },
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time assertion that `SessionClient: Send + Sync`, matching
    /// the contract documented on the `SessionClient` rustdoc
    /// (bd:JMAP-26di.41). If `JmapClient` or `Session` ever loses
    /// `Send` / `Sync` upstream, this stops the build instead of
    /// silently breaking the contract at consumer-build time.
    const _: fn() = || {
        fn _assert_send_sync<T: Send + Sync>() {}
        _assert_send_sync::<SessionClient>();
    };

    /// Oracle: `Patch::Keep` via `map_entry()` returns `None` (key omitted from patch).
    /// This is the canonical pattern used by all patch methods in this crate.
    /// The expected value `None` is derived directly from the spec: a field not
    /// present in the patch leaves the server value unchanged (RFC 8620 §5.3).
    #[test]
    fn patch_keep_via_map_entry() {
        let p: Patch<String> = Patch::Keep;
        let result = p.map_entry().expect("map_entry must not fail for Keep");
        assert!(
            result.is_none(),
            "Patch::Keep must produce None from map_entry (key omitted from patch)"
        );
    }

    /// Oracle: `Patch::Set(v)` via `map_entry()` returns `Some(json_value)`.
    /// Expected JSON is derived from the literal value "hello", not from the code.
    #[test]
    fn patch_set_via_map_entry() {
        let p = Patch::Set("hello".to_string());
        let result = p.map_entry().expect("map_entry must not fail for Set");
        assert_eq!(
            result,
            Some(serde_json::Value::String("hello".to_string())),
            "Patch::Set must produce Some(json_value) from map_entry"
        );
    }

    /// Oracle: `Patch::Clear` via `map_entry()` returns `Some(Value::Null)`.
    /// Clearing a nullable field sends explicit JSON null (RFC 8620 §5.3).
    #[test]
    fn patch_clear_via_map_entry() {
        let p: Patch<String> = Patch::Clear;
        let result = p.map_entry().expect("map_entry must not fail for Clear");
        assert_eq!(
            result,
            Some(serde_json::Value::Null),
            "Patch::Clear must produce Some(null) from map_entry"
        );
    }

    /// Oracle: `SpaceJoinInput::InviteCode` Debug must NOT contain the raw
    /// invite-code secret. The canary is a self-defined literal under the
    /// test's control, never derived from SpaceJoinInput's internal state.
    /// Same tripwire shape as the BearerAuth/BasicAuth redaction tests in
    /// jmap-base-client::auth (JMAP-sc1b.79).
    ///
    /// draft-atwood-jmap-chat-00 §4.18 defines the invite code as the
    /// unguessable bearer credential for Space/join.
    #[test]
    fn space_join_input_invite_code_debug_does_not_leak() {
        const CANARY: &str = "CANARY-JOIN-CODE-DO-NOT-LEAK-A1B2C3";
        let input = SpaceJoinInput::InviteCode(CANARY);
        let dbg = format!("{input:?}");
        assert!(
            !dbg.contains(CANARY),
            "SpaceJoinInput::InviteCode Debug must not contain the raw code; got: {dbg}"
        );
    }

    /// Oracle: `SpaceJoinInput::SpaceId` Debug renders the id verbatim — Id is
    /// not a secret per RFC 8620 §1.2, and existing diagnostic uses depend on
    /// the id being visible in logs. This is a positive assertion paired with
    /// the redaction test above to prove the redaction is variant-scoped.
    #[test]
    fn space_join_input_space_id_debug_shows_id() {
        let id = Id::from("s-public-space");
        let input = SpaceJoinInput::SpaceId(&id);
        let dbg = format!("{input:?}");
        assert!(
            dbg.contains("s-public-space"),
            "SpaceJoinInput::SpaceId Debug must expose the public id; got: {dbg}"
        );
    }

    /// Oracle: `PushSubscriptionPatch` Debug must NOT contain the raw
    /// `verification_code`. RFC 8620 §7.2 defines this value as the
    /// push-subscription ownership-proof secret; an attacker who learns it
    /// can hijack the subscription.
    #[test]
    fn push_subscription_patch_debug_does_not_leak_verification_code() {
        const CANARY: &str = "CANARY-VERIFICATION-CODE-DO-NOT-LEAK-D4E5F6";
        let patch = PushSubscriptionPatch {
            verification_code: Some(CANARY),
            ..PushSubscriptionPatch::default()
        };
        let dbg = format!("{patch:?}");
        assert!(
            !dbg.contains(CANARY),
            "PushSubscriptionPatch Debug must not contain the raw verification_code; got: {dbg}"
        );
        // Sanity: the field is still present in the Debug output as REDACTED,
        // so structural inspection (presence/absence of the field) still works.
        assert!(
            dbg.contains("verification_code"),
            "PushSubscriptionPatch Debug must still mention the verification_code field name; got: {dbg}"
        );
    }

    /// Oracle: `PushSubscriptionPatch` with `verification_code: None` renders
    /// as `None` (not `Some("[REDACTED]")`). Paired with the above, this
    /// proves the redaction does not corrupt the Some/None signal that a
    /// reader of the Debug output relies on.
    #[test]
    fn push_subscription_patch_debug_none_verification_code() {
        let patch = PushSubscriptionPatch::default();
        let dbg = format!("{patch:?}");
        assert!(
            dbg.contains("verification_code: None"),
            "PushSubscriptionPatch Debug with None verification_code must render as None; got: {dbg}"
        );
        assert!(
            !dbg.contains("REDACTED"),
            "PushSubscriptionPatch Debug with None verification_code must not show REDACTED; got: {dbg}"
        );
    }

    // ── Extras-preservation policy tests (JMAP-lbdy.9) ─────────────────
    //
    // Each test deserialises wire JSON containing a synthetic `acmeCorp*`
    // vendor field and asserts it survives in `extra`. The vendor field
    // names cannot collide with any field defined in RFC 8620 §7.2 or in
    // the draft-atwood-jmap-chat-00 method responses, so the tests are
    // independent of the code under test (workspace test-integrity rule).

    /// `PushSubscriptionCreateResponse.extra` captures unknown fields on deserialize.
    #[test]
    fn push_subscription_create_response_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "accountId": null,
            "created": {},
            "notCreated": {},
            "acmeCorpPushBackend": "fcm"
        });
        let obj: PushSubscriptionCreateResponse =
            serde_json::from_value(raw).expect("PushSubscriptionCreateResponse must deserialize");
        assert_eq!(
            obj.extra
                .get("acmeCorpPushBackend")
                .and_then(|v| v.as_str()),
            Some("fcm")
        );
    }

    /// `TypingResponse.extra` captures unknown fields on deserialize.
    #[test]
    fn typing_response_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "accountId": "acc1",
            "acmeCorpEchoLatencyMs": 12
        });
        let obj: TypingResponse =
            serde_json::from_value(raw).expect("TypingResponse must deserialize");
        assert_eq!(
            obj.extra
                .get("acmeCorpEchoLatencyMs")
                .and_then(|v| v.as_u64()),
            Some(12)
        );
    }

    /// `SpaceJoinResponse.extra` captures unknown fields on deserialize.
    #[test]
    fn space_join_response_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "accountId": "acc1",
            "spaceId": "S1",
            "acmeCorpWelcomeChannelId": "C-welcome"
        });
        let obj: SpaceJoinResponse =
            serde_json::from_value(raw).expect("SpaceJoinResponse must deserialize");
        assert_eq!(
            obj.extra
                .get("acmeCorpWelcomeChannelId")
                .and_then(|v| v.as_str()),
            Some("C-welcome")
        );
    }
}
