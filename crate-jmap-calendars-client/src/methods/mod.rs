//! Typed JMAP Calendars method wrappers — response types, [`SessionClient`],
//! constants, and helpers.
//!
//! Response types mirror RFC 8620 standard shapes (§5.1 /get, §5.5 /query,
//! §5.2 /changes, §5.3 /set). Method implementations live in sub-modules and
//! operate on `SessionClient`.

pub mod calendar;
pub mod event;
pub mod event_copy;
pub mod event_notification;
pub mod event_parse;
pub mod participant_identity;
pub mod principal_availability;

use std::collections::BTreeMap;

use serde::Deserialize;

use jmap_types::{Id, UTCDate};

// ---------------------------------------------------------------------------
// Response types (RFC 8620 §5)
// ---------------------------------------------------------------------------
//
// Re-exported from `jmap-types` so all `jmap-*-client` crates share
// one canonical set of /get, /set, /changes, /query, /queryChanges shapes.
// The wire format is identical to the previous local definitions.

pub use jmap_types::{
    AddedItem, ChangesResponse, CopyResponse, GetResponse, PartialObject, QueryChangesResponse,
    QueryResponse, SetError, SetResponse,
};

// ---------------------------------------------------------------------------
// CalendarEvent/parse response
// ---------------------------------------------------------------------------

/// Response type for `CalendarEvent/parse`
/// (draft-ietf-jmap-calendars-34 §5.13).
///
/// `parsed` maps each blob id to the list of `CalendarEvent` objects parsed
/// from that blob (a blob may contain multiple VEVENT components).
/// `not_found` lists blob ids that were not found in the account.
/// `not_parsable` lists blob ids that could not be parsed as iCalendar data.
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEventParseResponse {
    /// The account this response refers to.
    pub account_id: Id,
    /// Parsed `CalendarEvent` objects keyed by source blob id (a blob may
    /// contain multiple VEVENT components).
    pub parsed: Option<BTreeMap<Id, Vec<jmap_calendars_types::CalendarEvent>>>,
    /// Blob ids that could not be found in the account's blob store.
    pub not_found: Option<Vec<Id>>,
    /// Blob ids whose contents were not parseable as iCalendar data.
    pub not_parsable: Option<Vec<Id>>,
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
// Principal/getAvailability response
// ---------------------------------------------------------------------------

/// Response type for `Principal/getAvailability`
/// (draft-ietf-jmap-calendars-34 §2.2).
///
/// `list` contains the busy periods for the queried principal within the
/// requested time range.
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalGetAvailabilityResponse {
    /// The account this response refers to.
    pub account_id: Id,
    /// Busy periods for the queried principal within the requested time range.
    pub list: Vec<jmap_calendars_types::BusyPeriod>,
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
// Calendar/set extra parameters
// ---------------------------------------------------------------------------

/// Extra method-level arguments for `Calendar/set`
/// (draft-ietf-jmap-calendars-34 §4.4).
///
/// All fields are optional. Pass `None` (or `Default::default()`) when not
/// needed. The `if_in_state` optimistic-concurrency guard remains a
/// separate inline argument on
/// [`SessionClient::calendar_set`](SessionClient::calendar_set), matching
/// the canonical `email_submission_set` shape in `jmap-mail-client`.
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarSetParams {
    /// If `true`, destroying a calendar also destroys all its events
    /// (draft-ietf-jmap-calendars-34 §4.4). Server default: false (the
    /// server MUST reject a destroy on a calendar with events,
    /// returning the `calendarHasEvent` [`SetError`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_destroy_remove_events: Option<bool>,

    /// Calendar to make the account default once every create, update
    /// and destroy in the call succeeds (draft-ietf-jmap-calendars-34
    /// §4.3, `onSuccessSetIsDefault: Id|null`): an existing calendar
    /// id, or `"#"` followed by a creation id from this call's `create`
    /// map. A `String`, not [`Id`], because `#` is not a valid `Id`
    /// character. The server ignores an id it cannot find or a change
    /// it does not permit. A call with only this field set is valid
    /// (draft-ietf-jmap-calendars-34 §8.4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_success_set_is_default: Option<String>,

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
// CalendarEvent/set extra parameters
// ---------------------------------------------------------------------------

/// Extra method-level arguments for `CalendarEvent/set`
/// (draft-ietf-jmap-calendars-34 §5.9).
///
/// Pass `None` (or `Default::default()`) when not needed. Mirrors the
/// canonical `MailboxSetParams` shape in `jmap-mail-client`.
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEventSetParams {
    /// If `true`, the server sends iTIP scheduling messages for changes
    /// to scheduled events: to all participants when it is the origin of
    /// the event, otherwise back to the origin
    /// (draft-ietf-jmap-calendars-34 §5.9, §5.9.2). Server default:
    /// false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub send_scheduling_messages: Option<bool>,

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
// ParticipantIdentity/set extra parameters
// ---------------------------------------------------------------------------

/// Extra method-level arguments for `ParticipantIdentity/set`
/// (draft-ietf-jmap-calendars-34 §3.3).
///
/// Pass `None` (or `Default::default()`) when not needed. Mirrors the
/// canonical `MailboxSetParams` shape in `jmap-mail-client`.
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantIdentitySetParams {
    /// Identity to make the default once every create, update and
    /// destroy in the call succeeds (draft-ietf-jmap-calendars-34 §3.3,
    /// `onSuccessSetIsDefault: Id|null`): an existing identity id, or
    /// `"#"` followed by a creation id from this call's `create` map.
    /// A `String`, not [`Id`], because `#` is not a valid `Id`
    /// character. The server ignores an id it cannot find or a change
    /// it does not permit. A call with only this field set is valid.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_success_set_is_default: Option<String>,

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
// CalendarEvent/copy parameters
// ---------------------------------------------------------------------------

/// Arguments for `CalendarEvent/copy` (draft-ietf-jmap-calendars-34 §5.10).
///
/// Mirrors the canonical `EmailCopyParams` shape in `jmap-mail-client`
/// (workspace canonical extension-client template).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEventCopyParams {
    /// The account to copy from (RFC 8620 §5.4).
    pub from_account_id: Id,
    /// State guard for the source account: the server aborts with
    /// `stateMismatch` unless the source state matches (RFC 8620 §5.4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub if_from_in_state: Option<jmap_types::State>,
    /// State guard for the destination account: the server aborts with
    /// `stateMismatch` unless the destination state matches (RFC 8620 §5.4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub if_in_state: Option<jmap_types::State>,
    /// If `true`, destroy originals after successful copy (RFC 8620 §5.4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_success_destroy_original: Option<bool>,
    /// If-in-state guard for the source account destroy step (RFC 8620 §5.4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destroy_from_if_in_state: Option<jmap_types::State>,
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
// CalendarEvent/get extra parameters
// ---------------------------------------------------------------------------

/// Extra arguments accepted by `CalendarEvent/get`
/// (draft-ietf-jmap-calendars-34 §5.7).
///
/// All fields are optional. Pass `None` for any field to omit it from the
/// request (the server uses its default).
///
/// Mirrors the canonical [`EmailGetParams`] shape from `jmap-mail-client`:
/// derives `serde::Serialize` + `#[serde(rename_all = "camelCase")]` so
/// the wire shape is enforced by the type system, and carries an `extra`
/// flatten map for the workspace extras-preservation policy
/// (workspace AGENTS.md → "Extras-preservation policy" → in-scope:
/// "Method-argument structs in *-client crates"). Forward-compat for
/// future spec-defined fields is the `extra` map, not `#[non_exhaustive]`.
///
/// `expandRecurrences` is a `CalendarEvent/query` argument (§5.11), passed
/// to [`SessionClient::calendar_event_query`](crate::SessionClient::calendar_event_query).
///
/// [`EmailGetParams`]: https://docs.rs/jmap-mail-client/latest/jmap_mail_client/methods/struct.EmailGetParams.html
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEventGetParams {
    /// `recurrenceOverridesBefore` (`UTCDateTime|null`): only return
    /// `recurrenceOverrides` whose recurrence id is before this time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence_overrides_before: Option<UTCDate>,
    /// `recurrenceOverridesAfter` (`UTCDateTime|null`): only return
    /// `recurrenceOverrides` whose recurrence id is on or after this time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence_overrides_after: Option<UTCDate>,
    /// `reduceParticipants` (`Boolean`, default `false`): if `true`, only
    /// participants relevant to the user (the owner and the user's own
    /// participant) are returned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reduce_participants: Option<bool>,
    /// `timeZone` (`TimeZoneId`, default `"Etc/UTC"`): the time zone for
    /// floating events when computing `utcStart`/`utcEnd` and when
    /// translating recurrence ids for the two `recurrenceOverrides*`
    /// arguments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,
    /// Catch-all for vendor / site / private extension fields not
    /// covered by the typed fields above. Preserves unknown fields
    /// across deserialize/serialize round-trip per workspace
    /// extras-preservation policy.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Extra parameters accepted by `CalendarEvent/parse`
/// (draft-ietf-jmap-calendars-34 §5.13).
///
/// Mirrors the canonical [`EmailParseParams`] shape from
/// `jmap-mail-client`: optional spec-defined fields plus a `flatten`-ed
/// `extra` map for the workspace extras-preservation policy
/// (workspace AGENTS.md → "Extras-preservation policy" → in-scope:
/// "Method-argument structs in *-client crates").
///
/// All fields are optional. Pass `None` for any field to omit it from
/// the request (the server uses its default).
///
/// [`EmailParseParams`]: https://docs.rs/jmap-mail-client/latest/jmap_mail_client/methods/struct.EmailParseParams.html
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEventParseParams {
    /// Override the set of `CalendarEvent` properties returned per parsed
    /// blob. When `None`, the server returns the default property set
    /// documented in the draft.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<Vec<String>>,
    /// Catch-all for vendor / site / private extension fields not
    /// covered by the typed fields above. Preserves unknown fields
    /// across deserialize/serialize round-trip per workspace
    /// extras-preservation policy.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Extra parameters accepted by `Principal/getAvailability`
/// (draft-ietf-jmap-calendars-34 §2.2).
///
/// All fields are optional. Pass `None` for any field to omit it from
/// the request (the server uses its default).
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalGetAvailabilityParams {
    /// If `true`, the response includes per-`BusyPeriod` event detail
    /// (subject to per-event `mayReadItems` permission). When `None` or
    /// `Some(false)`, only opaque busy markers are returned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_details: Option<bool>,
    /// Subset of `CalendarEvent` properties to return on each
    /// `BusyPeriod`'s `event` field (only meaningful when
    /// `show_details: Some(true)`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_properties: Option<Vec<String>>,
    /// Catch-all for vendor / site / private extension fields not
    /// covered by the typed fields above. Preserves unknown fields
    /// across deserialize/serialize round-trip per workspace
    /// extras-preservation policy.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// The call-id embedded in every single-method JMAP request produced by
/// [`build_request`]. Pass directly to `jmap_base_client::extract_response`.
pub(crate) const CALL_ID: &str = "r1";

/// Capability URIs for JMAP Calendars method calls (draft-ietf-jmap-calendars-34).
pub(crate) const USING_CALENDARS: &[&str] = &[
    "urn:ietf:params:jmap:core",
    jmap_calendars_types::JMAP_CALENDARS_URI,
];

/// Capability URIs for `CalendarEvent/parse`
/// (draft-ietf-jmap-calendars-34 §5.13).
pub(crate) const USING_PARSE: &[&str] = &[
    "urn:ietf:params:jmap:core",
    jmap_calendars_types::JMAP_CALENDARS_URI,
    jmap_calendars_types::JMAP_CALENDARS_PARSE_URI,
];

/// Capability URIs for `Principal/getAvailability`
/// (draft-ietf-jmap-calendars-34 §2.2).
pub(crate) const USING_AVAILABILITY: &[&str] = &[
    "urn:ietf:params:jmap:core",
    jmap_calendars_types::JMAP_PRINCIPALS_AVAILABILITY_URI,
];

// ---------------------------------------------------------------------------
// build_request helper
// ---------------------------------------------------------------------------

/// Build a single-method JMAP request.
///
/// The embedded call-id is [`CALL_ID`]; pass it directly to
/// `jmap_base_client::extract_response`.
pub(crate) fn build_request(
    method: &str,
    args: serde_json::Value,
    using: &[&str],
) -> jmap_types::JmapRequest {
    let using_vec: Vec<String> = using.iter().map(|&s| s.to_owned()).collect();
    let invocation = jmap_types::Invocation::new(method, args, CALL_ID);
    jmap_types::JmapRequest::new(using_vec, vec![invocation], None)
}

// ---------------------------------------------------------------------------
// Id validation (RFC 8620 §1.2)
// ---------------------------------------------------------------------------
//
// Earlier revisions exposed `validate_id_field` / `validate_ids_field`
// helpers that wrapped `jmap_types::Id::new_validated` to guard the
// `&str` / `&[&str]` parameters (decision recorded on bd:JMAP-231o.6).
// The 0.2.0 typed-Id refactor (bd:JMAP-6by7) replaced those parameter
// shapes with `&Id` / `&[Id]` directly; once values reach the method
// body they are already validated by virtue of being typed `Id`s, so
// the helpers became dead code and were removed in bd:JMAP-6by7.1.
//
// Callers that still need ad-hoc validation (e.g. when building Ids
// from user input at a higher layer) should call
// `jmap_types::Id::new_validated` directly.

// ---------------------------------------------------------------------------
// SessionClient — session-bound client
// ---------------------------------------------------------------------------

/// A `JmapClient` bound to a JMAP session.
///
/// Obtain via [`JmapCalendarsExt::with_calendars_session`](crate::JmapCalendarsExt::with_calendars_session).
/// All JMAP Calendars methods are available on this type without needing to pass
/// `&Session` on every call.
///
/// `Clone` is derived because `JmapClient` is itself cheap-to-clone (it
/// already implements `Clone` and `with_calendars_session` clones one
/// internally), enabling parallel-task fan-out with one bound session.
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
    /// Set by [`SessionClient::with_i18n`].
    pub(crate) i18n: Option<jmap_base_client::I18nSetting>,
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

    /// Return the primary account id for `urn:ietf:params:jmap:calendars`.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    /// if the session has no primary account for that capability.
    pub fn calendars_account_id(&self) -> Result<&jmap_types::Id, jmap_base_client::ClientError> {
        self.session
            .primary_account_id(jmap_calendars_types::JMAP_CALENDARS_URI)
            .ok_or_else(|| {
                jmap_base_client::ClientError::InvalidSession(
                    "no primary account for urn:ietf:params:jmap:calendars".into(),
                )
            })
    }

    /// Extract `(api_url, calendars_account_id)` from the bound session.
    ///
    /// Returns `Err(InvalidSession)` if there is no primary account for
    /// `urn:ietf:params:jmap:calendars`.
    pub(crate) fn session_parts(&self) -> Result<(&str, &str), jmap_base_client::ClientError> {
        let api_url = self.session.api_url.as_str();
        let account_id = self
            .session
            .primary_account_id(jmap_calendars_types::JMAP_CALENDARS_URI)
            .ok_or_else(|| {
                jmap_base_client::ClientError::InvalidSession(
                    "no primary account for urn:ietf:params:jmap:calendars".into(),
                )
            })?;
        Ok((api_url, account_id.as_ref()))
    }

    /// Forward a JMAP request to the underlying HTTP client.
    pub(crate) async fn call_internal(
        &self,
        api_url: &str,
        req: &jmap_types::JmapRequest,
    ) -> Result<jmap_types::JmapResponse, jmap_base_client::ClientError> {
        match &self.i18n {
            Some(i18n) => {
                let mut req = req.clone();
                i18n.add_to_using(&mut req);
                self.client.call(api_url, &req).await
            }
            None => self.client.call(api_url, req).await,
        }
    }

    /// Use `urn:ietf:params:jmap:i18n` (draft-atwood-jmap-updates-00) on
    /// every call of this client (decision 32).
    ///
    /// The capability is added to `using`, so `Calendar` objects carry
    /// `locale` and `localizations` and `/set` may write them. With
    /// `language` set, `Calendar/get` sends it as the `language` argument
    /// and returns each Calendar's view in the best-matching language
    /// (RFC 4647 Lookup). `CalendarEvent` keeps JSCalendar's own
    /// `localizations` and takes no `language`. `None` returns the stored
    /// values with every localization.
    ///
    /// A client that caches objects by id and state MUST also key its cache
    /// on the language: views in different languages share a state. See
    /// [`jmap_base_client::I18nSetting`].
    ///
    /// # Errors
    ///
    /// [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    /// when the bound Session does not advertise the capability.
    pub fn with_i18n(
        mut self,
        language: Option<Vec<jmap_types::i18n::LanguageRange>>,
    ) -> Result<Self, jmap_base_client::ClientError> {
        self.i18n = Some(jmap_base_client::I18nSetting::new(&self.session, language)?);
        Ok(self)
    }

    /// Add the `language` argument set by [`Self::with_i18n`] to `args`.
    pub(crate) fn add_language(&self, args: &mut serde_json::Value) {
        if let Some(i18n) = &self.i18n {
            i18n.add_language(args);
        }
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Oracle: build_request produces the correct method name and CALL_ID.
    /// Expected values are literals from the spec, not derived from the function.
    #[test]
    fn build_request_method_name_and_call_id() {
        let req = build_request(
            "Calendar/get",
            json!({"accountId": "acc1", "ids": null}),
            USING_CALENDARS,
        );
        let v = serde_json::to_value(&req).expect("serialize JmapRequest");

        let calls = v["methodCalls"]
            .as_array()
            .expect("methodCalls must be array");
        assert_eq!(calls.len(), 1, "must have exactly 1 method call");
        assert_eq!(calls[0][0], json!("Calendar/get"), "method name must match");
        assert_eq!(calls[0][2], json!("r1"), "call_id must be CALL_ID constant");
    }

    /// Oracle: USING_CALENDARS contains exactly the two required capability URIs.
    /// Expected values are from draft-ietf-jmap-calendars-34 §1.
    #[test]
    fn using_calendars_contains_correct_uris() {
        let req = build_request("Calendar/get", json!({}), USING_CALENDARS);
        let v = serde_json::to_value(&req).expect("serialize");
        let using = v["using"].as_array().expect("using must be array");
        assert_eq!(using.len(), 2);
        assert!(
            using.contains(&json!("urn:ietf:params:jmap:core")),
            "must include jmap:core"
        );
        assert!(
            using.contains(&json!("urn:ietf:params:jmap:calendars")),
            "must include jmap:calendars"
        );
    }

    /// Oracle: SetResponse<T>.updated must accept null values per RFC 8620
    /// §5.3 wire type "Id[Foo|null]|null" (rfc8620.txt line 2043).
    ///
    /// The server returns null for a successfully updated object when the
    /// patch was applied verbatim with no server-set property deltas to
    /// report. A typed SetResponse<CalendarEvent> must deserialize this
    /// shape rather than failing because `null` cannot become CalendarEvent.
    ///
    /// Independent oracle: hand-written JSON fixture mirroring the spec
    /// wire shape directly — not generated by any code in this crate.
    #[test]
    fn set_response_updated_accepts_null_values() {
        let raw = json!({
            "accountId": "acc1",
            "oldState": "s1",
            "newState": "s2",
            "updated": {
                "ev1": null,
                "ev2": null
            }
        });
        let resp: SetResponse<jmap_calendars_types::CalendarEvent> = serde_json::from_value(raw)
            .expect("SetResponse must accept Id[Foo|null] per RFC 8620 §5.3");
        let updated = resp.updated;
        assert_eq!(updated.len(), 2, "two ids in updated map");
        assert!(
            updated
                .get(&Id::from("ev1"))
                .expect("ev1 key present")
                .is_none(),
            "ev1 value must be None (null)"
        );
        assert!(
            updated
                .get(&Id::from("ev2"))
                .expect("ev2 key present")
                .is_none(),
            "ev2 value must be None (null)"
        );
    }

    /// Oracle: SetResponse<T>.updated also accepts non-null Foo values per
    /// RFC 8620 §5.3 — the union "Id[Foo|null]" must round-trip both arms.
    /// Server returns a Foo object when server-set or computed properties
    /// changed beyond what the client patched (rfc8620.txt lines 2048-2051).
    #[test]
    fn set_response_updated_accepts_object_values() {
        let raw = json!({
            "accountId": "acc1",
            "oldState": "s1",
            "newState": "s2",
            "updated": {
                "ev1": { "id": "ev1", "title": "Meeting", "calendarIds": {"cal-1": true} }
            }
        });
        let resp: SetResponse<serde_json::Value> =
            serde_json::from_value(raw).expect("SetResponse must accept Id[Foo] per RFC 8620 §5.3");
        let updated = resp.updated;
        let ev1 = updated
            .get(&Id::from("ev1"))
            .expect("ev1 key present")
            .as_ref()
            .expect("ev1 value must be Some when server reports deltas");
        assert_eq!(ev1["title"], json!("Meeting"));
    }

    /// Oracle: RFC 8620 §5.3 — `notUpdated` and `notDestroyed` are
    /// `Id[SetError]` maps (server-assigned ids). The map keys MUST
    /// deserialize as `Id`, not `String`, so callers can use them
    /// interchangeably with ids from `/get` responses or the typed
    /// `destroyed` array.
    ///
    /// Independent oracle: hand-written JSON shaped per the spec wire
    /// definition; not derived from any code in this crate.
    #[test]
    fn set_response_not_updated_and_not_destroyed_keys_are_ids() {
        let raw = json!({
            "accountId": "acc1",
            "oldState": "s1",
            "newState": "s2",
            "notUpdated": {
                "ev-1": { "type": "stateMismatch" }
            },
            "notDestroyed": {
                "ev-2": { "type": "notFound" }
            }
        });
        let resp: SetResponse<serde_json::Value> = serde_json::from_value(raw)
            .expect("SetResponse must accept Id[SetError] per RFC 8620 §5.3");
        let not_updated = resp.not_updated;
        assert_eq!(
            not_updated
                .get(&Id::from("ev-1"))
                .expect("ev-1 key present")
                .error_type
                .to_string(),
            "stateMismatch",
            "notUpdated key must deserialize as Id, not String"
        );
        let not_destroyed = resp.not_destroyed;
        assert_eq!(
            not_destroyed
                .get(&Id::from("ev-2"))
                .expect("ev-2 key present")
                .error_type
                .to_string(),
            "notFound",
            "notDestroyed key must deserialize as Id, not String"
        );
    }

    /// Oracle: GetResponse<T> deserializes from RFC 8620 §5.1 shape.
    #[test]
    fn get_response_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "state": "s42",
            "list": [],
            "notFound": ["missing1"]
        });
        let resp: GetResponse<serde_json::Value> =
            serde_json::from_value(json).expect("GetResponse must deserialize");
        assert_eq!(resp.account_id, "acc1");
        assert_eq!(resp.state, "s42");
        assert!(resp.list.is_empty());
        assert_eq!(resp.not_found, [jmap_types::Id::from("missing1")]);
    }

    /// Oracle: CalendarEventParseResponse deserializes from a spec-derived JSON
    /// fixture (draft-ietf-jmap-calendars-34 §5.13).
    #[test]
    fn calendar_event_parse_response_deserializes() {
        let raw = json!({
            "accountId": "acc1",
            "parsed": { "blob1": [{"id": "ev1", "title": "Meeting"}] },
            "notFound": ["missing1"],
            "notParsable": []
        });
        let resp: CalendarEventParseResponse =
            serde_json::from_value(raw).expect("CalendarEventParseResponse must deserialize");
        assert_eq!(resp.account_id, "acc1");
        let parsed = resp.parsed.expect("parsed must be Some");
        let blob1_key = jmap_types::Id::from("blob1");
        assert!(parsed.contains_key(&blob1_key), "blob1 must be in parsed");
        assert_eq!(parsed[&blob1_key].len(), 1, "one event under blob1");
        assert_eq!(
            resp.not_found.as_deref(),
            Some(["missing1".into()].as_slice())
        );
        let not_parsable = resp.not_parsable.expect("notParsable must be Some");
        assert!(not_parsable.is_empty(), "notParsable must be empty");
    }

    /// Oracle: PrincipalGetAvailabilityResponse deserializes from a
    /// spec-derived JSON fixture (draft-ietf-jmap-calendars-34 §2.2;
    /// `busyStatus` "MUST be one of" confirmed, tentative, unavailable).
    #[test]
    fn principal_get_availability_response_deserializes() {
        let raw = json!({
            "accountId": "acc1",
            "list": [
                {
                    "utcStart": "2024-06-15T09:00:00Z",
                    "utcEnd": "2024-06-15T10:00:00Z",
                    "busyStatus": "confirmed",
                    "accountId": "acc1"
                }
            ]
        });
        let resp: PrincipalGetAvailabilityResponse =
            serde_json::from_value(raw).expect("PrincipalGetAvailabilityResponse must deserialize");
        assert_eq!(resp.account_id, "acc1");
        assert_eq!(resp.list.len(), 1, "one busy period");
        let period = &resp.list[0];
        assert_eq!(period.utc_start, "2024-06-15T09:00:00Z");
        assert_eq!(period.utc_end, "2024-06-15T10:00:00Z");
        assert_eq!(
            period.busy_status,
            jmap_calendars_types::BusyStatus::Confirmed,
            "busyStatus must be 'confirmed'"
        );
        assert_eq!(
            period.account_id.as_ref().map(|id| id.as_ref()),
            Some("acc1"),
            "accountId in BusyPeriod must be 'acc1'"
        );
    }

    // ── Extras-preservation policy tests (JMAP-lbdy.9) ─────────────────
    //
    // For Deserialize-only method-response structs, the test deserialises
    // JSON containing a vendor field and asserts the field is captured in
    // `extra`. The test uses synthetic `acmeCorp*` keys that are guaranteed
    // not to appear in any draft-defined field — so the tests are
    // independent of the crate under test.

    /// `CalendarEventParseResponse.extra` captures unknown fields on deserialize.
    #[test]
    fn calendar_event_parse_response_preserves_vendor_extras() {
        let raw = json!({
            "accountId": "acc1",
            "parsed": null,
            "notFound": null,
            "notParsable": null,
            "acmeCorpRequestId": "req-42"
        });
        let resp: CalendarEventParseResponse =
            serde_json::from_value(raw).expect("CalendarEventParseResponse must deserialize");
        assert_eq!(
            resp.extra.get("acmeCorpRequestId").and_then(|v| v.as_str()),
            Some("req-42")
        );
    }

    /// `PrincipalGetAvailabilityResponse.extra` captures unknown fields on deserialize.
    #[test]
    fn principal_get_availability_response_preserves_vendor_extras() {
        let raw = json!({
            "accountId": "acc1",
            "list": [],
            "acmeCorpQuotaRemaining": 99
        });
        let resp: PrincipalGetAvailabilityResponse =
            serde_json::from_value(raw).expect("PrincipalGetAvailabilityResponse must deserialize");
        assert_eq!(
            resp.extra
                .get("acmeCorpQuotaRemaining")
                .and_then(|v| v.as_i64()),
            Some(99)
        );
    }

    /// Oracle: draft-ietf-jmap-calendars-34 §5.7 argument names and
    /// types (`recurrenceOverridesBefore`/`After` UTCDateTime,
    /// `reduceParticipants` Boolean, `timeZone` TimeZoneId); `None`
    /// fields are absent.
    #[test]
    fn calendar_event_get_params_serializes() {
        let params = CalendarEventGetParams {
            recurrence_overrides_before: Some(UTCDate::from("2026-12-01T00:00:00Z")),
            reduce_participants: Some(true),
            time_zone: Some("Europe/Berlin".to_owned()),
            ..Default::default()
        };
        let out = serde_json::to_value(&params).expect("Serialize is infallible for plain data");
        assert_eq!(
            out,
            json!({
                "recurrenceOverridesBefore": "2026-12-01T00:00:00Z",
                "reduceParticipants": true,
                "timeZone": "Europe/Berlin"
            })
        );
    }

    /// Oracle: CalendarEventGetParams.extra carries vendor / site
    /// extension fields through the wire request via the flatten map.
    /// Workspace AGENTS.md mandates this for method-argument structs.
    #[test]
    fn calendar_event_get_params_extras_flatten() {
        let mut extra = serde_json::Map::new();
        extra.insert("acmeCorpDebug".to_owned(), json!(true));
        let params = CalendarEventGetParams {
            reduce_participants: Some(true),
            extra,
            ..Default::default()
        };
        let out = serde_json::to_value(&params).expect("Serialize is infallible for plain data");
        assert_eq!(out["reduceParticipants"], json!(true));
        assert_eq!(out["acmeCorpDebug"], json!(true));
    }

    /// Oracle: CalendarSetParams with on_destroy_remove_events serializes
    /// the field at the expected camelCase wire name.
    /// Expected field name "onDestroyRemoveEvents" from
    /// draft-ietf-jmap-calendars-34 §4.4.
    #[test]
    fn calendar_set_params_on_destroy_remove_events_serializes() {
        let params = CalendarSetParams {
            on_destroy_remove_events: Some(true),
            on_success_set_is_default: None,
            extra: serde_json::Map::new(),
        };
        let out = serde_json::to_value(&params).expect("serialize CalendarSetParams");
        assert_eq!(out["onDestroyRemoveEvents"], json!(true));
    }

    /// Oracle: CalendarSetParams default (all-None) serializes to an empty
    /// object — every typed field is `skip_serializing_if = "Option::is_none"`
    /// and `extra` is `skip_serializing_if = "Map::is_empty"`.
    #[test]
    fn calendar_set_params_default_serializes_empty() {
        let params = CalendarSetParams::default();
        let out = serde_json::to_value(&params).expect("serialize CalendarSetParams::default");
        let obj = out.as_object().expect("must be Object");
        assert!(
            obj.is_empty(),
            "all-None default must serialize to empty object, got: {out}"
        );
    }

    /// Oracle: draft-ietf-jmap-calendars-34 §8.4 — the
    /// default-calendar change request carries
    /// `"onSuccessSetIsDefault": "062adcfa-105d-455c-bc60-6db68b69c3f3"`.
    #[test]
    fn calendar_set_params_on_success_set_is_default_serializes() {
        let params = CalendarSetParams {
            on_success_set_is_default: Some("062adcfa-105d-455c-bc60-6db68b69c3f3".into()),
            ..Default::default()
        };
        let out = serde_json::to_value(&params).expect("serialize CalendarSetParams");
        assert_eq!(
            out,
            json!({"onSuccessSetIsDefault": "062adcfa-105d-455c-bc60-6db68b69c3f3"})
        );
    }

    /// Oracle: draft-ietf-jmap-calendars-34 §8.2, Figure 9 — the
    /// CalendarEvent/set request carries `"sendSchedulingMessages": true`.
    #[test]
    fn calendar_event_set_params_send_scheduling_messages_serializes() {
        let params = CalendarEventSetParams {
            send_scheduling_messages: Some(true),
            ..Default::default()
        };
        let out = serde_json::to_value(&params).expect("serialize CalendarEventSetParams");
        assert_eq!(out, json!({"sendSchedulingMessages": true}));
        let empty = serde_json::to_value(CalendarEventSetParams::default())
            .expect("serialize CalendarEventSetParams::default");
        assert_eq!(empty, json!({}));
    }

    /// Oracle: draft-ietf-jmap-calendars-34 §3.3 — a creation reference
    /// is "the creation id prefixed with a `#`", sent under
    /// `onSuccessSetIsDefault`.
    #[test]
    fn participant_identity_set_params_creation_reference_serializes() {
        let params = ParticipantIdentitySetParams {
            on_success_set_is_default: Some("#newPi".into()),
            ..Default::default()
        };
        let out = serde_json::to_value(&params).expect("serialize ParticipantIdentitySetParams");
        assert_eq!(out, json!({"onSuccessSetIsDefault": "#newPi"}));
        let empty = serde_json::to_value(ParticipantIdentitySetParams::default())
            .expect("serialize ParticipantIdentitySetParams::default");
        assert_eq!(empty, json!({}));
    }

    /// `CalendarSetParams.extra` flattens into serialized JSON.
    #[test]
    fn calendar_set_params_propagates_vendor_extras() {
        let mut params = CalendarSetParams::default();
        params
            .extra
            .insert("acmeCorpCascade".into(), json!("strict"));
        let v = serde_json::to_value(&params).expect("serialize CalendarSetParams");
        assert_eq!(v["acmeCorpCascade"], json!("strict"));
    }
}
