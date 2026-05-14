//! Shared backend infrastructure for all JMAP server crates.
//!
//! Re-exports the marker traits from `jmap-types` and adds the result types,
//! `BackendChangesError`, and [`JmapBackend`] supertrait. Domain crates add
//! their write-side methods and domain-specific error variants on top.

pub use jmap_types::{GetObject, JmapObject, QueryObject, SetObject};

// ---------------------------------------------------------------------------
// SetError — RFC 8620 §5.3 per-object set-method error
// ---------------------------------------------------------------------------

/// A per-item error in a `/set` response (`notCreated`, `notUpdated`,
/// `notDestroyed` maps) (RFC 8620 §5.3).
///
/// Construct with [`SetError::new`] and chain the builder methods as needed.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetError {
    /// The machine-readable error type.
    #[serde(rename = "type")]
    pub error_type: SetErrorType,
    /// Optional human-readable description of the error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Property names that caused the error (for `invalidProperties`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<Vec<String>>,
    /// The existing object id (for `alreadyExists` — RFC 8621 §5.7).
    #[serde(rename = "existingId", skip_serializing_if = "Option::is_none")]
    pub existing_id: Option<jmap_types::Id>,
    /// Maximum recipients allowed (for `tooManyRecipients` — RFC 8621 §7.5).
    #[serde(rename = "maxRecipients", skip_serializing_if = "Option::is_none")]
    pub max_recipients: Option<u64>,
    /// Invalid recipient addresses (for `invalidRecipients` — RFC 8621 §7.5).
    #[serde(rename = "invalidRecipients", skip_serializing_if = "Option::is_none")]
    pub invalid_recipients: Option<Vec<String>>,
    /// Missing blob IDs (for `blobNotFound` — RFC 8621 §5.5).
    #[serde(rename = "notFound", skip_serializing_if = "Option::is_none")]
    pub not_found: Option<Vec<jmap_types::Id>>,
    /// Maximum message size in octets (for `tooLarge` on EmailSubmission — RFC 8621 §7.5).
    #[serde(rename = "maxSize", skip_serializing_if = "Option::is_none")]
    pub max_size: Option<u64>,
    /// Catch-all for extension-defined SetError fields not covered by
    /// the typed members above.
    ///
    /// JMAP extensions sometimes ship error variants whose wire shape
    /// includes additional structured fields beyond the RFC 8620 §5.3
    /// base set — e.g. JMAP Chat's `rateLimited` SetError carries a
    /// `serverRetryAfter` UTCDate telling the client when it may
    /// retry, and `mdnAlreadySent` (RFC 8621 §7.7) is a typed
    /// extension error variant. This map preserves any such field
    /// across serialize / deserialize round-trip, mirroring the
    /// extras-preservation policy on the client-side
    /// [`jmap_types::SetError`] type.
    ///
    /// Use [`SetError::with_extra`] to populate from handler code:
    ///
    /// ```ignore
    /// SetError::new(SetErrorType::custom("rateLimited"))
    ///     .with_description("Slow mode is active for this chat")
    ///     .with_extra("serverRetryAfter", json!(retry_after_str))
    /// ```
    ///
    /// Per workspace AGENTS.md "Extras-preservation policy" — wire
    /// format is byte-identical to a pre-extras SetError when the
    /// map is empty (the `skip_serializing_if` collapses it).
    ///
    /// # Reserved-name invariant (bd:JMAP-jfia.17)
    ///
    /// Keys in [`RESERVED_SET_ERROR_WIRE_NAMES`] MUST NOT appear in
    /// this map. The typed fields above serialize to those names, so
    /// a colliding extras key produces a JSON object with two keys at
    /// the same level — RFC 8259 §4 permits duplicate keys but the
    /// behaviour is implementation-defined and the resulting SetError
    /// is malformed in practice.
    ///
    /// [`SetError::with_extra`] enforces this in debug builds via
    /// `debug_assert!`; direct field mutation (this field is `pub` per
    /// the workspace extras-preservation policy) bypasses that guard.
    /// Test and audit code SHOULD call [`SetError::validate_extras`]
    /// to detect collisions deterministically across build profiles.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl SetError {
    /// Construct a [`SetError`] with the given type and all optional fields `None`.
    pub fn new(error_type: SetErrorType) -> Self {
        Self {
            error_type,
            description: None,
            properties: None,
            existing_id: None,
            max_recipients: None,
            invalid_recipients: None,
            not_found: None,
            max_size: None,
            extra: serde_json::Map::new(),
        }
    }

    /// Set the human-readable description.
    ///
    /// # Security
    ///
    /// `SetError.description` is serialized verbatim into the JMAP wire
    /// response (RFC 8620 §5.3 `notCreated` / `notUpdated` /
    /// `notDestroyed` maps) and is visible to any client that can
    /// dispatch the failing `/set` call. The MUST-NOT rules that apply
    /// to [`JmapBackend::Error`]'s [`Display`](std::fmt::Display) output
    /// also apply to this string:
    ///
    /// - **Credential material** — auth tokens, passwords, push
    ///   verification codes, invite codes, session cookies, or anything
    ///   derived byte-for-byte from an `Authorization`-header value.
    /// - **Blob content** — email bodies, sieve scripts, file
    ///   contents, or any user-supplied opaque payload.
    /// - **PII shaped like an email address** in any code path that
    ///   an unauthenticated caller can trigger.
    ///
    /// Wrap downstream errors with [`crate::server_fail_from_backend`]
    /// (which always emits the static "internal error" description)
    /// rather than interpolating them into a SetError description.
    ///
    /// `SetError` paths are MORE leak-prone than `serverFail` because
    /// adversarial clients can probe for descriptions by sending
    /// crafted `/set` arguments — the typed-error contract guarantees
    /// the response includes a `SetError` for every failing target.
    /// Static, caller-meaningful descriptions ("rate limit exceeded —
    /// retry in N seconds", "patch nesting exceeds server limit") are
    /// fine; backend-error interpolations are not.
    ///
    /// Precedent: the parallel contract on
    /// [`JmapBackend::Error`] (bd:JMAP-sc1b.100) and the matching
    /// handler-side leak path closed in bd:JMAP-wlip.2. This warning
    /// added in bd:JMAP-wlip.26.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Set the list of property names that caused the error.
    pub fn with_properties<I, S>(mut self, props: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.properties = Some(props.into_iter().map(|s| s.into()).collect());
        self
    }

    /// Set the existing object id (used with `alreadyExists`).
    pub fn with_existing_id(mut self, id: jmap_types::Id) -> Self {
        self.existing_id = Some(id);
        self
    }

    /// Set the maximum recipients (used with `tooManyRecipients` — RFC 8621 §7.5).
    pub fn with_max_recipients(mut self, n: u64) -> Self {
        self.max_recipients = Some(n);
        self
    }

    /// Set the invalid recipient addresses (used with `invalidRecipients` — RFC 8621 §7.5).
    pub fn with_invalid_recipients<I, S>(mut self, addrs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.invalid_recipients = Some(addrs.into_iter().map(|s| s.into()).collect());
        self
    }

    /// Set the missing blob IDs (used with `blobNotFound` — RFC 8621 §5.5).
    pub fn with_not_found(mut self, ids: Vec<jmap_types::Id>) -> Self {
        self.not_found = Some(ids);
        self
    }

    /// Set the maximum message size in octets (used with `tooLarge` on EmailSubmission — RFC 8621 §7.5).
    pub fn with_max_size(mut self, n: u64) -> Self {
        self.max_size = Some(n);
        self
    }

    /// Insert an extension-defined field into [`Self::extra`].
    ///
    /// Used by handlers to attach typed wire fields that no `with_*`
    /// builder covers — for example JMAP Chat's `rateLimited` SetError
    /// must carry a `serverRetryAfter` UTCDate:
    ///
    /// ```ignore
    /// SetError::new(SetErrorType::custom("rateLimited"))
    ///     .with_description("Slow mode is active for this chat")
    ///     .with_extra("serverRetryAfter", serde_json::json!(retry_after_str))
    /// ```
    ///
    /// The serialized wire shape merges `key`/`value` at the same
    /// level as the typed fields (via `#[serde(flatten)]` on
    /// [`Self::extra`]). Calling `with_extra("type", ...)`,
    /// `with_extra("properties", ...)`, or any other reserved
    /// wire-name will produce a malformed SetError on the wire —
    /// callers are responsible for choosing extension-namespace keys
    /// that do not collide with the typed-field wire names.
    ///
    /// In debug builds, a `with_extra(key, ...)` call where `key` is in
    /// the reserved set [`RESERVED_SET_ERROR_WIRE_NAMES`] panics via
    /// `debug_assert!` to catch the bug at first test run
    /// (bd:JMAP-wlip.3). Release builds preserve the current
    /// no-validation behaviour to avoid silent runtime cost on a
    /// correctly-written caller.
    pub fn with_extra(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        // bd:JMAP-jfia.32 — accept impl Into<String> to match the
        // sibling builders on the same type (with_description,
        // with_properties, with_invalid_recipients). Existing
        // &str-passing call sites compile unchanged via
        // impl From<&str> for String.
        let key: String = key.into();
        debug_assert!(
            !RESERVED_SET_ERROR_WIRE_NAMES.contains(&key.as_str()),
            "SetError::with_extra called with reserved wire-name key {key:?} \
             — would produce a malformed JSON SetError on the wire. \
             Choose an extension-namespace key that does not collide \
             with the typed-field wire names \
             ({RESERVED_SET_ERROR_WIRE_NAMES:?})."
        );
        self.extra.insert(key, value);
        self
    }

    /// Validate that [`Self::extra`] does not contain any key in
    /// [`RESERVED_SET_ERROR_WIRE_NAMES`] (bd:JMAP-jfia.17).
    ///
    /// [`Self::with_extra`] enforces the same invariant in debug builds
    /// via `debug_assert!`, but direct field mutation (e.g.
    /// `err.extra.insert("type", json!("evil"))`) bypasses that guard.
    /// This method is the deterministic, build-profile-independent
    /// gate: callers and tests that construct SetError values
    /// programmatically should run it before serializing, to catch
    /// the collision case that would produce a malformed wire shape
    /// with two keys at the same name.
    ///
    /// Returns the first colliding key on `Err`; check `validate_extras`
    /// in a loop if you need to surface all collisions.
    ///
    /// # Errors
    ///
    /// Returns [`ReservedExtrasKey`] with the first reserved key
    /// encountered in [`Self::extra`].
    pub fn validate_extras(&self) -> Result<(), ReservedExtrasKey> {
        for key in self.extra.keys() {
            if RESERVED_SET_ERROR_WIRE_NAMES.contains(&key.as_str()) {
                return Err(ReservedExtrasKey { key: key.clone() });
            }
        }
        Ok(())
    }
}

/// Returned by [`SetError::validate_extras`] when [`SetError::extra`]
/// contains a key that collides with a typed-field wire-name in
/// [`RESERVED_SET_ERROR_WIRE_NAMES`] (bd:JMAP-jfia.17).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReservedExtrasKey {
    /// The first reserved wire-name found in `SetError.extra`.
    pub key: String,
}

impl std::fmt::Display for ReservedExtrasKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SetError.extra contains reserved wire-name key {:?} — would \
             produce a malformed JSON SetError on the wire",
            self.key
        )
    }
}

impl std::error::Error for ReservedExtrasKey {}

/// Reserved wire-name keys that [`SetError::with_extra`] MUST NOT receive.
///
/// These are the JSON keys emitted by the typed `#[serde(rename)]` /
/// `#[serde(rename_all = "camelCase")]` fields on [`SetError`]. Passing
/// any of these as the `key` argument to `with_extra` would produce a
/// JSON object with two keys at the same name — technically RFC 8259
/// §4 permits duplicate keys but the behaviour is implementation-defined
/// and the resulting SetError on the wire is malformed in practice.
///
/// Kept here as a `pub const` rather than inline in the assert message
/// so consumers can reference the same list — e.g. a future contract
/// test, or a wire-format conformance check.
pub const RESERVED_SET_ERROR_WIRE_NAMES: &[&str] = &[
    "type",
    "description",
    "properties",
    "existingId",
    "maxRecipients",
    "invalidRecipients",
    "notFound",
    "maxSize",
];

impl std::fmt::Display for SetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.error_type)?;
        if let Some(ref desc) = self.description {
            write!(f, ": {desc}")?;
        }
        Ok(())
    }
}

/// The machine-readable type for a [`SetError`] (RFC 8620 §5.3 and RFC 8621).
///
/// # Variant policy
///
/// The variant set below carries:
///
/// - The 10 RFC 8620 §5.3 base error types
///   (`Forbidden`, `OverQuota`, `TooLarge`, `RateLimit`, `NotFound`,
///   `InvalidPatch`, `WillDestroy`, `InvalidProperties`, `Singleton`,
///   `AlreadyExists`).
/// - 13 RFC 8621 mail-specific error types
///   (`MailboxHasChild`, `MailboxHasEmail`, `TooManyKeywords`,
///   `TooManyMailboxes`, `BlobNotFound`, `ForbiddenFrom`,
///   `InvalidEmail`, `TooManyRecipients`, `NoRecipients`,
///   `InvalidRecipients`, `ForbiddenMailFrom`, `ForbiddenToSend`,
///   `CannotUnsend`). These predate the canonical-template extraction
///   and ship in the foundation for back-compat with existing
///   `jmap-mail-server` callers (bd:JMAP-wlip.19).
/// - [`Self::Custom`] for everything else.
///
/// **New extension errors MUST use [`Self::custom`].** Other JMAP
/// extensions (chat, calendars, tasks, contacts, filenode, sharing,
/// metadata) ship their error strings via `custom("rateLimited")`,
/// `custom("addressBookHasContents")`, `custom("invalidSieve")`, etc.
/// The known wire-name table inside the private `from_wire_str` helper
/// is the authoritative list of typed variants — any wire-name outside
/// that list round-trips as `Custom(s)`.
///
/// The mail-variants asymmetry is documented but not yet reshaped.
/// Moving the 13 mail variants to `jmap-mail-types` is a breaking
/// change that requires a workspace-wide major version bump and
/// propagation across every `*-server` extension crate; it is tracked
/// separately rather than performed silently. Until that bump, do not
/// add further extension-specific variants here — even mail-style
/// extensions like Calendars / Tasks / Contacts use [`Self::custom`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum SetErrorType {
    /// The action would violate an ACL or other access control policy.
    Forbidden,
    /// Creating or modifying the object would exceed a server quota.
    OverQuota,
    /// The object is too large to be stored by the server.
    TooLarge,
    /// The server is rate-limiting this client.
    RateLimit,
    /// The object to be updated or destroyed does not exist.
    NotFound,
    /// The patch object is not a valid JSON Merge Patch or cannot be applied.
    InvalidPatch,
    /// The client requested destruction of an object that will be destroyed
    /// implicitly when another object is destroyed.
    WillDestroy,
    /// One or more properties have invalid values.
    InvalidProperties,
    /// The object type is a singleton and cannot be created or destroyed.
    Singleton,
    /// An object with the same unique key already exists.
    AlreadyExists,
    /// RFC 8621 §2.5 — Mailbox has child mailboxes and cannot be destroyed.
    MailboxHasChild,
    /// RFC 8621 §2.5 — Mailbox contains emails and `onDestroyRemoveEmails` is false.
    MailboxHasEmail,
    /// RFC 8621 §5.5 — Too many keywords on the Email.
    TooManyKeywords,
    /// RFC 8621 §5.5 — Email is in too many mailboxes.
    TooManyMailboxes,
    /// RFC 8621 §5.5 — A referenced blob was not found.
    BlobNotFound,
    /// RFC 8621 §6.3 — The `from` address is not permitted for this Identity.
    ForbiddenFrom,
    /// RFC 8621 §7.5 — The Email is invalid for submission.
    InvalidEmail,
    /// RFC 8621 §7.5 — Too many recipients.
    TooManyRecipients,
    /// RFC 8621 §7.5 — No recipients specified.
    NoRecipients,
    /// RFC 8621 §7.5 — One or more recipient addresses are invalid.
    InvalidRecipients,
    /// RFC 8621 §7.5 — The MAIL FROM address is not permitted.
    ForbiddenMailFrom,
    /// RFC 8621 §7.5 — The user does not have send permission.
    ForbiddenToSend,
    /// RFC 8621 §7.5 — The submission cannot be undone.
    CannotUnsend,
    /// An extension-defined error type not covered by the variants above.
    /// Serializes as the inner string directly (e.g. `"mdnAlreadySent"`).
    Custom(String),
}

impl SetErrorType {
    /// Construct a [`SetErrorType`] from any string, canonicalising
    /// known wire-names back to their typed variant.
    ///
    /// `custom("forbidden")` returns [`SetErrorType::Forbidden`], NOT
    /// `Custom("forbidden")`. Only strings that do not match any known
    /// JMAP wire-name produce [`SetErrorType::Custom`]. This makes
    /// round-trip symmetric — `custom(s)` equals the result of
    /// deserialising `"s"` for every `s`, eliminating the silent
    /// contract drift filed as bd:JMAP-wlip.22.
    ///
    /// Use this in extension crates to emit domain-specific error
    /// types without adding variants to this enum; if your extension's
    /// chosen name later becomes a typed variant in this crate, the
    /// call site keeps working — `custom("mdnAlreadySent")` returns
    /// `Custom("mdnAlreadySent")` today and would return the typed
    /// variant when that variant is added.
    pub fn custom(s: impl Into<String>) -> Self {
        let s: String = s.into();
        Self::from_wire_str(&s).unwrap_or(Self::Custom(s))
    }

    /// Map a JMAP wire-name string to its typed variant, returning
    /// `None` for any string not in the known-name set.
    ///
    /// Single source of truth used by both [`Self::custom`] and the
    /// [`serde::Deserialize`] visitor (bd:JMAP-wlip.22). Adding a new
    /// typed variant requires extending this match arm AND the
    /// `Display` impl; the table-driven round-trip test
    /// `set_error_type_all_known_variants_round_trip` (bd:JMAP-wlip.29)
    /// catches any drift between them.
    fn from_wire_str(s: &str) -> Option<Self> {
        Some(match s {
            "forbidden" => Self::Forbidden,
            "overQuota" => Self::OverQuota,
            "tooLarge" => Self::TooLarge,
            "rateLimit" => Self::RateLimit,
            "notFound" => Self::NotFound,
            "invalidPatch" => Self::InvalidPatch,
            "willDestroy" => Self::WillDestroy,
            "invalidProperties" => Self::InvalidProperties,
            "singleton" => Self::Singleton,
            "alreadyExists" => Self::AlreadyExists,
            "mailboxHasChild" => Self::MailboxHasChild,
            "mailboxHasEmail" => Self::MailboxHasEmail,
            "tooManyKeywords" => Self::TooManyKeywords,
            "tooManyMailboxes" => Self::TooManyMailboxes,
            "blobNotFound" => Self::BlobNotFound,
            "forbiddenFrom" => Self::ForbiddenFrom,
            "invalidEmail" => Self::InvalidEmail,
            "tooManyRecipients" => Self::TooManyRecipients,
            "noRecipients" => Self::NoRecipients,
            "invalidRecipients" => Self::InvalidRecipients,
            "forbiddenMailFrom" => Self::ForbiddenMailFrom,
            "forbiddenToSend" => Self::ForbiddenToSend,
            "cannotUnsend" => Self::CannotUnsend,
            _ => return None,
        })
    }
}

impl std::fmt::Display for SetErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s: &str = match self {
            Self::Forbidden => "forbidden",
            Self::OverQuota => "overQuota",
            Self::TooLarge => "tooLarge",
            Self::RateLimit => "rateLimit",
            Self::NotFound => "notFound",
            Self::InvalidPatch => "invalidPatch",
            Self::WillDestroy => "willDestroy",
            Self::InvalidProperties => "invalidProperties",
            Self::Singleton => "singleton",
            Self::AlreadyExists => "alreadyExists",
            Self::MailboxHasChild => "mailboxHasChild",
            Self::MailboxHasEmail => "mailboxHasEmail",
            Self::TooManyKeywords => "tooManyKeywords",
            Self::TooManyMailboxes => "tooManyMailboxes",
            Self::BlobNotFound => "blobNotFound",
            Self::ForbiddenFrom => "forbiddenFrom",
            Self::InvalidEmail => "invalidEmail",
            Self::TooManyRecipients => "tooManyRecipients",
            Self::NoRecipients => "noRecipients",
            Self::InvalidRecipients => "invalidRecipients",
            Self::ForbiddenMailFrom => "forbiddenMailFrom",
            Self::ForbiddenToSend => "forbiddenToSend",
            Self::CannotUnsend => "cannotUnsend",
            Self::Custom(s) => s.as_str(),
        };
        f.write_str(s)
    }
}

impl serde::Serialize for SetErrorType {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        // bd:JMAP-jfia.33 — collect_str avoids the per-call String
        // allocation that `s.serialize_str(&self.to_string())` does.
        // serde_json's collect_str uses a stack buffer for short
        // strings; every SetErrorType variant's Display is short
        // enough to fit. The round-trip oracle
        // set_error_type_all_known_variants_round_trip pins
        // wire-format identity.
        s.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for SetErrorType {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = SetErrorType;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JMAP SetError type string")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                // Single source of truth shared with [`SetErrorType::custom`]
                // (bd:JMAP-wlip.22). An unknown wire-name falls through to
                // Custom; a known wire-name canonicalises to its typed
                // variant so that round-trip is symmetric.
                Ok(SetErrorType::from_wire_str(v)
                    .unwrap_or_else(|| SetErrorType::Custom(v.to_owned())))
            }
        }
        d.deserialize_str(Visitor)
    }
}

/// Error type returned by create/update/destroy backend methods.
#[non_exhaustive]
#[derive(Debug)]
pub enum BackendSetError<E> {
    /// A well-typed JMAP [`SetError`] to place verbatim in the
    /// `notCreated`/`notUpdated`/`notDestroyed` map.
    SetError(SetError),
    /// An unexpected storage-layer error.
    Other(E),
}

impl<E: std::fmt::Display> std::fmt::Display for BackendSetError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SetError(se) => write!(f, "set error: {se}"),
            Self::Other(e) => write!(f, "{e}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for BackendSetError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Other(e) => Some(e),
            _ => None,
        }
    }
}

impl<E> From<SetError> for BackendSetError<E> {
    fn from(e: SetError) -> Self {
        Self::SetError(e)
    }
}

// ---------------------------------------------------------------------------
// Backend error envelopes
// ---------------------------------------------------------------------------

/// Error type returned by [`JmapBackend::get_changes`] and
/// [`JmapBackend::query_changes`].
///
/// # `CannotCalculate` vs `TooManyChanges`
///
/// The two non-`Other` variants map to two distinct JMAP wire errors
/// (RFC 8620 §5.6). Previously a single `TooManyChanges { limit: 0 }`
/// variant overloaded both meanings via a magic-zero sentinel; the
/// `CannotCalculate` variant was added in bd:JMAP-jfia.31 to surface
/// the distinction at the type level. `TooManyChanges { limit: 0 }`
/// is preserved as a deprecated alias — it still maps to
/// `cannotCalculateChanges` via the `From` and `Display` impls — but
/// new backends SHOULD construct `CannotCalculate` directly.
#[non_exhaustive]
#[derive(Debug)]
pub enum BackendChangesError<E> {
    /// The server has no usable change log for the given `sinceState`
    /// and cannot supply incremental changes — the client MUST
    /// discard ALL locally cached objects for the affected type,
    /// reset its local state token to the empty string, and perform a
    /// full resync (`/get` with `ids: null`). Partial recovery is not
    /// permitted. Maps to `cannotCalculateChanges` (RFC 8620 §5.6;
    /// authoritative behavior documented in jmapio/jmap-js
    /// `mail-model.js`).
    ///
    /// Added in bd:JMAP-jfia.31 to replace the
    /// `TooManyChanges { limit: 0 }` magic-zero overload. New backends
    /// SHOULD construct `CannotCalculate` directly; legacy backends
    /// that emit `TooManyChanges { limit: 0 }` still map to the same
    /// wire error via the deprecation path.
    CannotCalculate,
    /// The change window exceeds what the server can supply in a
    /// single `/changes` response. Maps to `tooManyChanges` with the
    /// `limit` as the suggested maximum — the client may retry with
    /// a smaller window.
    ///
    /// **Deprecated sub-case (bd:JMAP-jfia.31)**: a `limit` of `0`
    /// historically meant "full state reset required" and is
    /// preserved as an alias for the new [`Self::CannotCalculate`]
    /// variant. New code SHOULD use `CannotCalculate` directly; the
    /// alias may be removed at the next major-version bump.
    TooManyChanges {
        /// Maximum window size the server can supply in a single
        /// `/changes` response. A value of `0` is the deprecated
        /// alias for [`Self::CannotCalculate`]; any non-zero value
        /// is the suggested maximum the client may retry with.
        limit: u64,
    },
    /// An unexpected storage-layer error.
    Other(E),
}

impl<E: std::fmt::Display> std::fmt::Display for BackendChangesError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CannotCalculate => write!(f, "cannot calculate changes"),
            // Deprecated magic-zero alias (bd:JMAP-jfia.31).
            Self::TooManyChanges { limit: 0 } => write!(f, "cannot calculate changes"),
            Self::TooManyChanges { limit } => write!(f, "too many changes (limit: {limit})"),
            Self::Other(e) => write!(f, "{e}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for BackendChangesError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Other(e) => Some(e),
            _ => None,
        }
    }
}

impl<E> From<E> for BackendChangesError<E> {
    fn from(e: E) -> Self {
        Self::Other(e)
    }
}

impl<E: std::error::Error> From<BackendChangesError<E>> for jmap_types::JmapError {
    fn from(e: BackendChangesError<E>) -> Self {
        match e {
            BackendChangesError::CannotCalculate => {
                jmap_types::JmapError::cannot_calculate_changes()
            }
            // Deprecated magic-zero alias for CannotCalculate
            // (bd:JMAP-jfia.31). Preserved so legacy backends that
            // emit TooManyChanges { limit: 0 } continue to produce
            // the correct wire error.
            BackendChangesError::TooManyChanges { limit: 0 } => {
                jmap_types::JmapError::cannot_calculate_changes()
            }
            BackendChangesError::TooManyChanges { limit } => {
                jmap_types::JmapError::too_many_changes_with_limit(limit)
            }
            // bd:JMAP-jfia.1 / bd:JMAP-wlip.2 — the `Other` arm wraps a
            // backend `Error` whose `Display` impl is contractually
            // forbidden from carrying credential/blob/PII text but which
            // we still treat as untrusted at the wire boundary. Use the
            // same static [`SERVER_FAIL_INTERNAL_DESC`] string that the
            // [`server_fail_from_backend`] handler-layer helper uses so
            // the defence-in-depth chain (backend Display contract →
            // handler helper → this From impl) cannot be bypassed by
            // handlers that take the ergonomic `.map_err(JmapError::from)?`
            // path on `BackendChangesError`.
            //
            // [`SERVER_FAIL_INTERNAL_DESC`]: crate::handlers::SERVER_FAIL_INTERNAL_DESC
            // [`server_fail_from_backend`]: crate::handlers::server_fail_from_backend
            BackendChangesError::Other(_inner) => {
                jmap_types::JmapError::server_fail(crate::handlers::SERVER_FAIL_INTERNAL_DESC)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Result of a `/changes` call (RFC 8620 §5.2).
#[derive(Debug)]
#[non_exhaustive]
pub struct ChangesResult {
    /// Ids of objects that were created since `sinceState`.
    pub created: Vec<jmap_types::Id>,
    /// Ids of objects that were updated since `sinceState`.
    pub updated: Vec<jmap_types::Id>,
    /// Ids of objects that were destroyed since `sinceState`.
    pub destroyed: Vec<jmap_types::Id>,
    /// `true` if there are more changes beyond this batch.
    pub has_more_changes: bool,
    /// The current state token after applying all reported changes.
    pub new_state: jmap_types::State,
}

impl ChangesResult {
    /// Construct a [`ChangesResult`].
    pub fn new(
        created: Vec<jmap_types::Id>,
        updated: Vec<jmap_types::Id>,
        destroyed: Vec<jmap_types::Id>,
        has_more_changes: bool,
        new_state: jmap_types::State,
    ) -> Self {
        Self {
            created,
            updated,
            destroyed,
            has_more_changes,
            new_state,
        }
    }
}

/// Result of a `/query` call (RFC 8620 §5.5).
#[derive(Debug)]
#[non_exhaustive]
pub struct QueryResult {
    /// The ordered list of matching object ids.
    pub ids: Vec<jmap_types::Id>,
    /// The 0-based index of the first returned id in the complete result list.
    ///
    /// RFC 8620 §5.5 specifies this as `UnsignedInt` in the response —
    /// a non-negative integer (bd:JMAP-wlip.25). The request-side
    /// position parameter accepts negative values as end-relative
    /// offsets, but the response position cannot validly be negative.
    /// Backends that derive `position` from a request-side `i64`
    /// offset MUST clamp / normalize to `u64` before constructing this
    /// struct.
    pub position: u64,
    /// Total number of results, if the backend can calculate it.
    pub total: Option<u64>,
    /// Opaque query state token for subsequent `/queryChanges` calls.
    pub query_state: jmap_types::State,
    /// Whether the backend supports `/queryChanges` for this query.
    pub can_calculate_changes: bool,
}

impl QueryResult {
    /// Construct a [`QueryResult`].
    pub fn new(
        ids: Vec<jmap_types::Id>,
        position: u64,
        total: Option<u64>,
        query_state: jmap_types::State,
        can_calculate_changes: bool,
    ) -> Self {
        Self {
            ids,
            position,
            total,
            query_state,
            can_calculate_changes,
        }
    }
}

/// One entry in the `added` list of a `/queryChanges` response (RFC 8620 §5.6).
#[derive(Debug)]
#[non_exhaustive]
pub struct AddedItem {
    /// The id of the newly-added object.
    pub id: jmap_types::Id,
    /// Its 0-based position in the result list after applying all changes.
    pub index: u64,
}

impl AddedItem {
    /// Construct an [`AddedItem`].
    pub fn new(id: jmap_types::Id, index: u64) -> Self {
        Self { id, index }
    }
}

/// Result of a `/queryChanges` call (RFC 8620 §5.6).
#[derive(Debug)]
#[non_exhaustive]
pub struct QueryChangesResult {
    /// The query state token supplied by the client in `sinceQueryState`.
    pub old_query_state: jmap_types::State,
    /// The current query state token.
    pub new_query_state: jmap_types::State,
    /// Total number of results in the new query, if the backend can calculate it.
    pub total: Option<u64>,
    /// Ids removed from the result set since `oldQueryState`.
    pub removed: Vec<jmap_types::Id>,
    /// Ids added to the result set since `oldQueryState`, with their positions.
    pub added: Vec<AddedItem>,
}

impl QueryChangesResult {
    /// Construct a [`QueryChangesResult`].
    pub fn new(
        old_query_state: jmap_types::State,
        new_query_state: jmap_types::State,
        total: Option<u64>,
        removed: Vec<jmap_types::Id>,
        added: Vec<AddedItem>,
    ) -> Self {
        Self {
            old_query_state,
            new_query_state,
            total,
            removed,
            added,
        }
    }
}

// ---------------------------------------------------------------------------
// JmapBackend — the read-side supertrait
// ---------------------------------------------------------------------------

/// Read-side backend supertrait shared by all JMAP server crates.
///
/// Domain-specific backend traits (`MailBackend`, `ChatBackend`, etc.) require
/// this trait as a supertrait and add write-side methods on top.
///
/// Only the read operations that have an identical signature across all JMAP
/// object types belong here. Write operations (`create_object`, `update_object`,
/// `destroy_object`) and domain-specific operations remain in the domain crate.
///
/// The `collapse_threads` parameter on `query_changes` is included for
/// `Email/queryChanges` (RFC 8621 §4.5). Non-mail backends should pass `false`
/// and may ignore the parameter.
///
/// This trait is not object-safe by design (generic methods). Use
/// `Arc<impl JmapBackend>` when sharing across tasks.
///
/// # CallerCtx
///
/// Every backend method takes a `caller: &Self::CallerCtx` parameter as the
/// first argument after `&self`. This is the per-request authentication /
/// authorisation context produced by the caller's auth layer and forwarded
/// unchanged through [`crate::Dispatcher::dispatch`] → [`crate::JmapHandler`]
/// → the registered closure → the backend.
///
/// Implementations that do not need an auth identity can use the unit type:
///
/// ```rust,ignore
/// impl JmapBackend for MyBackend {
///     type Error = MyError;
///     type CallerCtx = ();
///     // ...
/// }
/// ```
///
/// Implementations that do need to differentiate behaviour per caller (e.g.
/// applying per-user visibility rules, or rejecting reads with
/// `forbidden` when the caller is not the owner of the account) read the
/// `caller` parameter to decide.
///
/// The trait bound `Clone + Send + 'static` is what [`crate::Dispatcher`]
/// requires; the bound is repeated here so the supertrait can stand on its
/// own without depending on the dispatcher.
pub trait JmapBackend: Send + Sync + 'static {
    /// The error type returned by storage operations.
    ///
    /// # Security
    ///
    /// The `Display` impl of this type is surfaced through
    /// [`BackendSetError::Other`]'s and [`BackendChangesError::Other`]'s
    /// own `Display` impls, which in turn flow into
    /// [`crate::request_error`]'s `RequestError::Display` output. When a
    /// downstream consumer wires tracing-style logging on top, the
    /// formatted error text lands in operator logs verbatim.
    ///
    /// Implementations MUST NOT include any of the following in this
    /// type's `Display` output:
    ///
    /// - **Credential material** — auth tokens, passwords, push
    ///   verification codes, invite codes, session cookies, or anything
    ///   derived byte-for-byte from an `Authorization`-header value.
    /// - **Blob content** — email bodies, sieve scripts, file contents,
    ///   or any user-supplied opaque payload. An error like
    ///   `"sieve parse error at line 42: <script excerpt>"` violates
    ///   this — emit the line number and a short type-only summary
    ///   ("sieve parse error at line 42: unexpected token") and let the
    ///   server log the full script body separately under a redacted
    ///   path.
    /// - **PII shaped like an email address** in any code path that an
    ///   unauthenticated caller can trigger. Wrapping a downstream
    ///   service error that interpolates the caller's email is the
    ///   common foot-gun.
    ///
    /// Errors that wrap a downstream-service failure should sanitize
    /// the downstream error text — or strip it entirely and replace it
    /// with a static summary — before constructing the `Display`
    /// string. The same rule applies to every extension `*Backend`
    /// trait that inherits this associated type by transitivity:
    /// `MailBackend::Error`, `ChatBackend::Error`,
    /// `CalendarsBackend::Error`, `TasksBackend::Error`,
    /// `ContactsBackend::Error`, `FileNodeBackend::Error`, and
    /// `SharingBackend::Error` are all the same `JmapBackend::Error`
    /// associated type — the contract here governs all of them.
    ///
    /// Precedent: bd:JMAP-sc1b.79 redacted `BearerAuth` and `BasicAuth`
    /// at the type-derive level; bd:JMAP-sc1b.100 documents the
    /// equivalent contract at the trait-associated-type level.
    type Error: std::error::Error + Send + Sync + 'static;

    /// The per-request caller context type produced by the auth layer and
    /// forwarded by [`crate::Dispatcher::dispatch`] into every method call.
    ///
    /// Use `()` when no auth context is needed.
    ///
    /// The bound is `Clone + Send + Sync + 'static`:
    /// - `Clone` because [`crate::Dispatcher`] clones the value once per
    ///   method call in the batch.
    /// - `Send + 'static` because each method call is spawned on a
    ///   [`tokio::task`].
    /// - `Sync` because handler method bodies take `&Self::CallerCtx`
    ///   and hold that reference across `.await` boundaries inside a
    ///   `Send` future (a `&T` is `Send` iff `T: Sync`).
    type CallerCtx: Clone + Send + Sync + 'static;

    /// Return `true` if the given account exists in this backend.
    ///
    /// Handlers call this at the start of each method to return
    /// `accountNotFound` (RFC 8620 §3.6.2) rather than surfacing
    /// the wrong error when `accountId` is unknown.
    fn account_exists(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send;

    /// Fetch objects by id (or all objects when `ids` is `None`).
    ///
    /// `properties` is the list of property names requested by the client
    /// (RFC 8620 §5.1). `None` means the client did not send a `properties`
    /// field; the backend should return all properties. When `Some`, the backend
    /// MAY filter the response to only the named properties, but is not required
    /// to — implementations that always return all properties are correct.
    ///
    /// Returns `(found, not_found)` — objects that exist and ids that do not.
    fn get_objects<O: GetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        ids: Option<&[jmap_types::Id]>,
        properties: Option<&[String]>,
    ) -> impl std::future::Future<Output = Result<(Vec<O>, Vec<jmap_types::Id>), Self::Error>> + Send;

    /// Return the current state token for an object type in the given account.
    fn get_state<O: JmapObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<jmap_types::State, Self::Error>> + Send;

    /// Return changes since `since_state`, up to `max_changes` entries.
    fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        since_state: &jmap_types::State,
        max_changes: Option<u64>,
    ) -> impl std::future::Future<Output = Result<ChangesResult, BackendChangesError<Self::Error>>> + Send;

    /// Execute a `/query` and return a page of matching ids.
    ///
    /// `position` may be negative — negative values are relative to the end of
    /// the result set per RFC 8620 §5.5 (e.g. -1 means the last result).
    ///
    /// # Filter and sort handling
    ///
    /// Implementations MUST honour the supplied `filter` and `sort` arguments
    /// efficiently — typically by pushing both into the indexed storage layer
    /// (database WHERE / ORDER BY, search index, etc.). Returning every
    /// matching id and relying on the caller to paginate after the fact
    /// degenerates to O(n) per page for IMAP-migration accounts.
    ///
    /// Handler implementations in `jmap-*-server` crates SHOULD NOT
    /// post-filter or post-sort the backend's result; doing so re-introduces
    /// the O(n) cost this method exists to avoid. The Mailbox handler in
    /// `jmap-mail-server` is the canonical example of pushing filter/sort
    /// fully into the backend.
    #[allow(clippy::too_many_arguments)]
    fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        position: i64,
    ) -> impl std::future::Future<Output = Result<QueryResult, Self::Error>> + Send;

    /// Execute a `/queryChanges` and return deltas since `since_query_state`.
    ///
    /// `collapse_threads` is only meaningful for `Email/queryChanges`
    /// (RFC 8621 §4.5). Pass `false` for all other object types.
    #[allow(clippy::too_many_arguments)]
    fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        since_query_state: &jmap_types::State,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        max_changes: Option<u64>,
        up_to_id: Option<&jmap_types::Id>,
        collapse_threads: bool,
    ) -> impl std::future::Future<
        Output = Result<QueryChangesResult, BackendChangesError<Self::Error>>,
    > + Send;

    /// The caller's stable identity within this account namespace.
    ///
    /// Returns `None` for deployments that have not wired identity
    /// (test fixtures, single-user dev servers). A `None`-returning
    /// backend CANNOT honor JMAP semantics that depend on caller
    /// identity — chat role-hierarchy, calendar ACLs, sharing/myRights,
    /// per-user $seen on shared mailboxes, metadata isPrivate
    /// visibility scoping, etc. Authentication is still the HTTP
    /// layer's job; this method exposes the result of that
    /// authentication to the JMAP layer for in-method semantics.
    ///
    /// Implementations MUST NOT mint identity — they MUST read it
    /// from the `CallerCtx` populated by the HTTP/auth middleware
    /// before `dispatch()` was called.
    ///
    /// Backends that honor identity-dependent semantics MUST override
    /// this method. Handlers and downstream backend traits MAY rely on
    /// it being correct when it returns `Some`.
    ///
    /// # Why an associated function and not a method (bd:JMAP-wlip.6)
    ///
    /// The signature deliberately takes `caller: &Self::CallerCtx`
    /// without a `&self` receiver. Backends therefore have no access
    /// to their own storage state from inside `principal_id`. The
    /// auth-layer middleware MUST pre-resolve the principal (e.g. map
    /// a JWT `sub` claim to a local `Id` via an internal lookup) and
    /// stash the result inside `CallerCtx` *before* it calls
    /// `dispatch`. The JMAP layer reads the pre-resolved value here;
    /// no JIT lookup is possible.
    ///
    /// This is a structural enforcement of the "identity is not the
    /// JMAP layer's job to mint" rule. A consumer that wants
    /// JIT-resolved identity (e.g. database-backed JWT → principal
    /// mapping) wires that mapping into the HTTP layer's `CallerCtx`
    /// construction step instead of trying to fit it inside the
    /// backend's `principal_id` impl.
    fn principal_id(caller: &Self::CallerCtx) -> Option<&jmap_types::Id> {
        let _ = caller;
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle: BackendChangesError::TooManyChanges { limit: 0 } must map to
    /// cannotCalculateChanges (RFC 8620 §5.6), not tooManyChanges with limit 0.
    ///
    /// limit=0 is the convention for "cannot calculate".
    #[test]
    fn backend_changes_error_limit_zero_maps_to_cannot_calculate() {
        let err = jmap_types::JmapError::from(
            BackendChangesError::<std::convert::Infallible>::TooManyChanges { limit: 0 },
        );
        assert_eq!(
            err.error_type.as_str(),
            "cannotCalculateChanges",
            "limit=0 must produce cannotCalculateChanges; got: {:?}",
            err.error_type
        );
    }

    /// Oracle (bd:JMAP-jfia.31): the new `CannotCalculate` variant
    /// maps to `cannotCalculateChanges` on the wire, matching the
    /// deprecated `TooManyChanges { limit: 0 }` alias. New backends
    /// SHOULD emit `CannotCalculate` directly.
    #[test]
    fn backend_changes_error_cannot_calculate_maps_to_cannot_calculate_changes() {
        let err = jmap_types::JmapError::from(
            BackendChangesError::<std::convert::Infallible>::CannotCalculate,
        );
        assert_eq!(
            err.error_type.as_str(),
            "cannotCalculateChanges",
            "CannotCalculate must produce cannotCalculateChanges; got: {:?}",
            err.error_type
        );

        // Display agrees with the deprecated-alias Display arm.
        let s = BackendChangesError::<std::convert::Infallible>::CannotCalculate.to_string();
        assert_eq!(
            s, "cannot calculate changes",
            "Display must produce the same string as TooManyChanges {{ limit: 0 }}"
        );
    }

    /// Oracle: BackendChangesError::TooManyChanges { limit: N } (N > 0) maps to
    /// tooManyChanges with the suggested limit.
    #[test]
    fn backend_changes_error_nonzero_limit_maps_to_too_many_changes() {
        let err = jmap_types::JmapError::from(
            BackendChangesError::<std::convert::Infallible>::TooManyChanges { limit: 50 },
        );
        assert_eq!(
            err.error_type.as_str(),
            "tooManyChanges",
            "limit=50 must produce tooManyChanges; got: {:?}",
            err.error_type
        );
    }

    /// Oracle (bd:JMAP-jfia.1 / bd:JMAP-wlip.2): the
    /// `From<BackendChangesError<E>> for JmapError` impl MUST NOT echo
    /// the wrapped backend error's `Display` text into the resulting
    /// `JmapError`'s description. The defence-in-depth contract is that
    /// even if a backend implementor accidentally violates the
    /// [`JmapBackend::Error`] Display MUST-NOT (credential / blob /
    /// PII), the leaked text never reaches the wire — and the
    /// ergonomic `.map_err(JmapError::from)?` path that
    /// `handle_changes` / `handle_query_changes` take on
    /// `BackendChangesError` must redact identically to the explicit
    /// `server_fail_from_backend(&e)` helper used elsewhere.
    ///
    /// Test vector: an `Other` variant whose Display contains a canary
    /// string resembling a credential leak. The canary literal is
    /// hand-built and not derived from any production type's
    /// behaviour. Mirrors
    /// `server_fail_from_backend_drops_display_text` in
    /// `handlers.rs`.
    #[test]
    fn backend_changes_error_other_drops_display_text() {
        #[derive(Debug)]
        struct LeakyError(&'static str);
        impl std::fmt::Display for LeakyError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.0)
            }
        }
        impl std::error::Error for LeakyError {}

        const CANARY: &str = "TOKEN-DO-NOT-LEAK-c0ffee";
        let err: BackendChangesError<LeakyError> = BackendChangesError::Other(LeakyError(CANARY));

        let jmap_err = jmap_types::JmapError::from(err);

        // Serialize to wire shape and assert the canary is absent from
        // the resulting JSON. The error_invocation wraps a JmapError as
        // { "type": "serverFail", "description": "..." } — both fields
        // are wire-visible.
        let wire = serde_json::to_value(&jmap_err).expect("JmapError must serialize");
        let wire_str = wire.to_string();
        assert!(
            !wire_str.contains(CANARY),
            "From<BackendChangesError<E>> for JmapError must not echo \
             backend error Display onto the wire; got {wire_str}"
        );
        // The description MUST be exactly SERVER_FAIL_INTERNAL_DESC.
        assert_eq!(
            wire["description"],
            crate::handlers::SERVER_FAIL_INTERNAL_DESC,
            "description must be the static 'internal error' string"
        );
        assert_eq!(wire["type"], "serverFail");
    }

    /// Oracle (bd:JMAP-wlip.22): `SetErrorType::custom("forbidden")` MUST
    /// return `SetErrorType::Forbidden`, not `Custom("forbidden")`. The
    /// asymmetry where `custom("forbidden") != Forbidden` was a real
    /// foot-gun: handler code intending to emit the typed variant via
    /// the `custom` builder produced a Custom that was wire-identical
    /// but PartialEq-distinct, breaking test assertions that compared
    /// the deserialised round-trip against the typed expected value.
    ///
    /// Test vector: every known typed variant name canonicalises to its
    /// typed variant; an unknown name stays Custom. The wire-name list
    /// is hand-built from the same RFC source as the round-trip test
    /// `set_error_type_all_known_variants_round_trip`.
    #[test]
    fn custom_canonicalises_known_wire_names_to_typed_variants() {
        // Spot-check a representative subset across the 23 known names.
        // The exhaustive round-trip from from_wire_str is exercised by
        // set_error_type_all_known_variants_round_trip; this test focuses
        // on the custom() → typed-variant direction the bead was about.
        let cases: &[(&str, SetErrorType)] = &[
            ("forbidden", SetErrorType::Forbidden),
            ("overQuota", SetErrorType::OverQuota),
            ("invalidPatch", SetErrorType::InvalidPatch),
            ("mailboxHasChild", SetErrorType::MailboxHasChild),
            ("tooManyRecipients", SetErrorType::TooManyRecipients),
            ("cannotUnsend", SetErrorType::CannotUnsend),
        ];
        for (name, expected) in cases {
            let from_custom = SetErrorType::custom(*name);
            assert_eq!(
                &from_custom, expected,
                "custom({name:?}) must canonicalise to the typed variant, not Custom"
            );
            assert!(
                !matches!(from_custom, SetErrorType::Custom(_)),
                "custom({name:?}) must NOT remain Custom — known wire-name asymmetry"
            );
        }

        // Unknown names stay Custom — extension crates depend on this.
        let unknown = SetErrorType::custom("mdnAlreadySent");
        assert!(
            matches!(unknown, SetErrorType::Custom(ref s) if s == "mdnAlreadySent"),
            "custom('mdnAlreadySent') must remain Custom (not a known wire-name)"
        );
    }

    /// Oracle: SetErrorType::Custom("mdnAlreadySent") must serialize as the bare
    /// string "mdnAlreadySent" and deserialize back to Custom("mdnAlreadySent").
    /// Extension crates depend on this round-trip to emit domain-specific errors.
    #[test]
    fn set_error_type_custom_round_trips_as_bare_string() {
        let original = SetErrorType::custom("mdnAlreadySent");
        let serialized = serde_json::to_string(&original).expect("serialize");
        assert_eq!(
            serialized, r#""mdnAlreadySent""#,
            "Custom must serialize as bare string"
        );
        let deserialized: SetErrorType = serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(
            deserialized, original,
            "Custom must deserialize back to Custom"
        );
    }

    /// Oracle (bd:JMAP-dha0): SetError gains an `extra` map that captures
    /// extension-defined fields not covered by the typed `with_*` builders.
    /// A handler that emits `rateLimited` with `serverRetryAfter` must
    /// see the value round-trip through serialize / deserialize.
    #[test]
    fn set_error_extra_field_round_trips() {
        let original = SetError::new(SetErrorType::custom("rateLimited"))
            .with_description("Slow mode is active")
            .with_extra(
                "serverRetryAfter",
                serde_json::Value::String("2025-12-31T23:59:59Z".to_owned()),
            );

        let wire = serde_json::to_value(&original).expect("serialize");
        assert_eq!(wire["type"], "rateLimited");
        assert_eq!(wire["description"], "Slow mode is active");
        assert_eq!(
            wire["serverRetryAfter"], "2025-12-31T23:59:59Z",
            "extra field must flatten into the SetError wire shape"
        );

        let round: SetError = serde_json::from_value(wire).expect("deserialize");
        assert_eq!(round.error_type, original.error_type);
        assert_eq!(round.description, original.description);
        assert_eq!(
            round.extra.get("serverRetryAfter").and_then(|v| v.as_str()),
            Some("2025-12-31T23:59:59Z"),
            "extra field must survive deserialize"
        );
    }

    /// Oracle (bd:JMAP-dha0): a SetError with no extras serializes to a
    /// wire shape byte-identical to the pre-extras layout. The
    /// `skip_serializing_if` on `extra` collapses the empty map.
    #[test]
    fn set_error_empty_extra_is_invisible_on_the_wire() {
        let err = SetError::new(SetErrorType::Forbidden);
        let wire = serde_json::to_value(&err).expect("serialize");
        let obj = wire.as_object().expect("object");
        assert!(
            !obj.contains_key("extra"),
            "empty `extra` map must not appear on the wire (got {wire})"
        );
        // The only key on the wire for a bare SetError must be `type`.
        assert_eq!(
            obj.len(),
            1,
            "bare SetError must have exactly one key on the wire"
        );
        assert_eq!(obj["type"], "forbidden");
    }

    /// Oracle (bd:JMAP-dha0): unknown wire fields on a deserialized
    /// SetError land in `extra`. This means a future spec adding
    /// `someNewSetErrorField` will round-trip through current
    /// versions of the kit losslessly.
    #[test]
    fn set_error_unknown_field_lands_in_extra() {
        let wire = serde_json::json!({
            "type": "forbidden",
            "futureSpecField": "future-value",
            "anotherOne": 42
        });
        let err: SetError = serde_json::from_value(wire).expect("deserialize");
        assert_eq!(err.error_type, SetErrorType::Forbidden);
        assert_eq!(
            err.extra.get("futureSpecField").and_then(|v| v.as_str()),
            Some("future-value")
        );
        assert_eq!(
            err.extra.get("anotherOne").and_then(|v| v.as_u64()),
            Some(42)
        );
    }

    /// Oracle (bd:JMAP-wlip.3): [`SetError::with_extra`] panics in debug
    /// builds when called with a reserved wire-name key. Catches the bug
    /// at first test run rather than letting a malformed-on-the-wire
    /// SetError ship through review. The assert is debug-only so release
    /// builds pay no runtime cost on correctly-written callers.
    ///
    /// Iterates every wire-name in [`RESERVED_SET_ERROR_WIRE_NAMES`] so
    /// adding a new typed field to SetError plus its rename to the
    /// constant list keeps the negative tests in sync automatically.
    #[test]
    #[cfg(debug_assertions)]
    fn with_extra_panics_on_reserved_wire_name() {
        for &reserved in RESERVED_SET_ERROR_WIRE_NAMES {
            let reserved_owned = reserved.to_owned();
            let result = std::panic::catch_unwind(move || {
                SetError::new(SetErrorType::Forbidden)
                    .with_extra(&reserved_owned, serde_json::Value::Null);
            });
            assert!(
                result.is_err(),
                "with_extra({reserved:?}, ...) must panic in debug builds; \
                 reserved wire-names collide with typed fields and would \
                 produce a malformed SetError on the wire"
            );
        }
    }

    /// Oracle (bd:JMAP-jfia.17): direct mutation of `SetError.extra`
    /// bypasses the `with_extra` debug_assert and can plant a
    /// reserved wire-name. [`SetError::validate_extras`] is the
    /// deterministic, build-profile-independent gate for the same
    /// invariant.
    ///
    /// Test vector: iterate every reserved wire-name, plant it
    /// directly into `extra`, and assert `validate_extras` returns
    /// `Err(ReservedExtrasKey { key: <name> })`.
    #[test]
    fn validate_extras_detects_reserved_key_planted_via_direct_mutation() {
        for &reserved in RESERVED_SET_ERROR_WIRE_NAMES {
            let mut err = SetError::new(SetErrorType::Forbidden);
            // Bypass with_extra entirely — this is the pattern the
            // `pub` field surface invites that the debug_assert cannot
            // see (bd:JMAP-jfia.17).
            err.extra
                .insert(reserved.to_owned(), serde_json::Value::Null);
            let collision = err
                .validate_extras()
                .expect_err("reserved-name extras key must be detected");
            assert_eq!(
                collision.key, reserved,
                "validate_extras must report the colliding key verbatim"
            );
        }
    }

    /// Oracle (bd:JMAP-jfia.17): `validate_extras` returns `Ok(())` for
    /// a SetError whose `extra` map contains only extension-namespace
    /// keys. Positive control paired with the rejection test above.
    #[test]
    fn validate_extras_accepts_extension_namespace_keys() {
        let mut err = SetError::new(SetErrorType::custom("rateLimited"));
        err.extra.insert(
            "serverRetryAfter".to_owned(),
            serde_json::Value::String("2025-12-31T23:59:59Z".to_owned()),
        );
        err.extra
            .insert("retryAttempt".to_owned(), serde_json::Value::from(3));
        err.validate_extras()
            .expect("extension-namespace keys must pass validation");
    }

    /// Oracle (bd:JMAP-wlip.3): a non-reserved key passes the
    /// [`SetError::with_extra`] debug_assert and lands in the `extra`
    /// map as before. Positive control paired with the panic test
    /// above.
    #[test]
    fn with_extra_accepts_extension_namespace_key() {
        // 'serverRetryAfter' is the JMAP Chat extension's rateLimited
        // SetError field; not in RESERVED_SET_ERROR_WIRE_NAMES.
        let err = SetError::new(SetErrorType::custom("rateLimited")).with_extra(
            "serverRetryAfter",
            serde_json::Value::String("2025-12-31T23:59:59Z".to_owned()),
        );
        assert_eq!(
            err.extra.get("serverRetryAfter").and_then(|v| v.as_str()),
            Some("2025-12-31T23:59:59Z"),
            "extension-namespace key must land in the extra map"
        );
    }

    /// Oracle (bd:JMAP-wlip.3): the reserved-name constant covers every
    /// `#[serde(rename = ...)]` and camelCase-derived field name on the
    /// public [`SetError`] surface. A future contributor that adds a
    /// typed wire field without extending the constant is the failure
    /// mode this test guards against. The oracle is hand-derived from
    /// the SetError struct definition by reading off the wire-name of
    /// each field.
    #[test]
    fn reserved_set_error_wire_names_matches_serialized_surface() {
        // Build a SetError with every typed field populated, serialize,
        // and check that every JSON key (other than the extension extras)
        // appears in RESERVED_SET_ERROR_WIRE_NAMES.
        let err = SetError::new(SetErrorType::Forbidden)
            .with_description("desc")
            .with_properties(["p1"])
            .with_existing_id(jmap_types::Id::from("eid"))
            .with_max_recipients(10)
            .with_invalid_recipients(["bad@example"])
            .with_not_found(vec![jmap_types::Id::from("nfid")])
            .with_max_size(1024);

        let wire = serde_json::to_value(&err).expect("serialize");
        let obj = wire.as_object().expect("SetError must serialize as object");
        for key in obj.keys() {
            assert!(
                RESERVED_SET_ERROR_WIRE_NAMES.contains(&key.as_str()),
                "wire-name {key:?} appears on the SetError surface but is \
                 not in RESERVED_SET_ERROR_WIRE_NAMES — adding a typed \
                 field to SetError requires extending the constant"
            );
        }
    }

    /// Oracle (bd:JMAP-wlip.29): the Display arm, the Deserialize visitor
    /// match, and the round-trip behaviour MUST agree for every known
    /// variant of [`SetErrorType`]. The mapping is duplicated across three
    /// places (Display, Serialize via Display, Deserialize visitor); the
    /// workspace dep allowlist forbids strum / serde_with that would
    /// derive the round-trip from a single source. This table-driven
    /// test iterates ALL 23 typed variants and asserts:
    ///
    ///   - Display produces the expected camelCase wire string
    ///   - serde_json::to_string emits the same string
    ///   - serde_json::from_str rebuilds the same variant (NOT a Custom)
    ///
    /// A drift between Display and Deserialize (e.g. adding "rateLimit"
    /// to Display but forgetting the Deserialize arm) would fail step 3
    /// at first test run because the wire string would round-trip into
    /// `Custom("rateLimit")` instead of `RateLimit`. This is the silent
    /// contract drift filed as bd:JMAP-wlip.22.
    ///
    /// The table is hand-built from the RFC 8620 / RFC 8621 spec text,
    /// not derived from the code under test (the workspace test-integrity
    /// rule requires an independent oracle). Adding a new typed variant
    /// requires extending this table — that is the intent.
    #[test]
    fn set_error_type_all_known_variants_round_trip() {
        // (variant constructor, expected wire string).
        // Source of truth: RFC 8620 §5.3 + RFC 8621 §2.5, §5.5, §6.3, §7.5.
        let cases: &[(SetErrorType, &str)] = &[
            (SetErrorType::Forbidden, "forbidden"),
            (SetErrorType::OverQuota, "overQuota"),
            (SetErrorType::TooLarge, "tooLarge"),
            (SetErrorType::RateLimit, "rateLimit"),
            (SetErrorType::NotFound, "notFound"),
            (SetErrorType::InvalidPatch, "invalidPatch"),
            (SetErrorType::WillDestroy, "willDestroy"),
            (SetErrorType::InvalidProperties, "invalidProperties"),
            (SetErrorType::Singleton, "singleton"),
            (SetErrorType::AlreadyExists, "alreadyExists"),
            (SetErrorType::MailboxHasChild, "mailboxHasChild"),
            (SetErrorType::MailboxHasEmail, "mailboxHasEmail"),
            (SetErrorType::TooManyKeywords, "tooManyKeywords"),
            (SetErrorType::TooManyMailboxes, "tooManyMailboxes"),
            (SetErrorType::BlobNotFound, "blobNotFound"),
            (SetErrorType::ForbiddenFrom, "forbiddenFrom"),
            (SetErrorType::InvalidEmail, "invalidEmail"),
            (SetErrorType::TooManyRecipients, "tooManyRecipients"),
            (SetErrorType::NoRecipients, "noRecipients"),
            (SetErrorType::InvalidRecipients, "invalidRecipients"),
            (SetErrorType::ForbiddenMailFrom, "forbiddenMailFrom"),
            (SetErrorType::ForbiddenToSend, "forbiddenToSend"),
            (SetErrorType::CannotUnsend, "cannotUnsend"),
        ];

        for (variant, expected_wire) in cases {
            // Display
            assert_eq!(
                variant.to_string(),
                *expected_wire,
                "Display arm for {variant:?} produced wrong wire string"
            );
            // Serialize (delegates to Display)
            let serialized = serde_json::to_string(variant).expect("serialize");
            assert_eq!(
                serialized,
                format!("\"{expected_wire}\""),
                "Serialize for {variant:?} did not produce \"{expected_wire}\""
            );
            // Deserialize back — MUST rebuild the typed variant, NOT Custom.
            let deserialized: SetErrorType =
                serde_json::from_str(&serialized).expect("deserialize");
            assert_eq!(
                &deserialized, variant,
                "Deserialize of {expected_wire:?} did not rebuild {variant:?} \
                 (likely fell through to Custom — Display and Deserialize \
                 match arms have drifted)"
            );
            // Belt-and-braces: explicitly assert NOT Custom.
            assert!(
                !matches!(deserialized, SetErrorType::Custom(_)),
                "Deserialize of {expected_wire:?} fell through to Custom; \
                 Display has an arm but Deserialize visitor doesn't"
            );
        }
    }

    /// Oracle (bd:JMAP-ga0q.1): `JmapBackend::principal_id` has a default impl
    /// that returns `None`. A backend whose `CallerCtx = ()` and that does NOT
    /// override `principal_id` inherits that default and signals "identity not
    /// wired" to callers. JMAP semantics that depend on caller identity must
    /// treat `None` as a hard "cannot honor".
    #[test]
    fn principal_id_default_impl_returns_none_for_unit_caller_ctx() {
        // Minimal stub backend exercising only the default impl. All other
        // trait methods are stubbed with `unreachable!()` and never invoked.
        struct StubBackend;

        #[derive(Debug)]
        struct StubError;

        impl std::fmt::Display for StubError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("stub")
            }
        }
        impl std::error::Error for StubError {}

        impl JmapBackend for StubBackend {
            type Error = StubError;
            type CallerCtx = ();

            async fn account_exists(
                &self,
                _caller: &(),
                _account_id: &jmap_types::Id,
            ) -> Result<bool, Self::Error> {
                unreachable!("only principal_id is exercised in this test")
            }

            async fn get_objects<O: GetObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &jmap_types::Id,
                _ids: Option<&[jmap_types::Id]>,
                _properties: Option<&[String]>,
            ) -> Result<(Vec<O>, Vec<jmap_types::Id>), Self::Error> {
                unreachable!("only principal_id is exercised in this test")
            }

            async fn get_state<O: JmapObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &jmap_types::Id,
            ) -> Result<jmap_types::State, Self::Error> {
                unreachable!("only principal_id is exercised in this test")
            }

            async fn get_changes<O: JmapObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &jmap_types::Id,
                _since_state: &jmap_types::State,
                _max_changes: Option<u64>,
            ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
                unreachable!("only principal_id is exercised in this test")
            }

            async fn query_objects<O: QueryObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &jmap_types::Id,
                _filter: Option<&O::Filter>,
                _sort: Option<&[O::Comparator]>,
                _limit: Option<u64>,
                _position: i64,
            ) -> Result<QueryResult, Self::Error> {
                unreachable!("only principal_id is exercised in this test")
            }

            async fn query_changes<O: QueryObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &jmap_types::Id,
                _since_query_state: &jmap_types::State,
                _filter: Option<&O::Filter>,
                _sort: Option<&[O::Comparator]>,
                _max_changes: Option<u64>,
                _up_to_id: Option<&jmap_types::Id>,
                _collapse_threads: bool,
            ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
                unreachable!("only principal_id is exercised in this test")
            }
        }

        let caller: <StubBackend as JmapBackend>::CallerCtx = ();
        let id = <StubBackend as JmapBackend>::principal_id(&caller);
        assert!(
            id.is_none(),
            "default principal_id impl must return None; got Some({:?})",
            id
        );
    }
}
