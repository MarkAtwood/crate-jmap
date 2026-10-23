//! RFC 8620 §5.3 per-object `/set` error ([`SetError`], [`SetErrorType`]).

use serde::{Deserialize, Serialize};

use crate::Id;

/// A per-item error in a `/set` response (`notCreated`, `notUpdated`,
/// `notDestroyed` maps) (RFC 8620 §5.3).
///
/// One type serves both sides: servers construct it with [`SetError::new`]
/// and the builder methods, clients decode it from [`SetResponse`]. Unknown
/// extension error types decode as [`SetErrorType::Other`] and unknown
/// extension fields land in [`Self::extra`], so both round-trip losslessly.
///
/// [`SetResponse`]: crate::SetResponse
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    pub existing_id: Option<Id>,
    /// Maximum recipients allowed (for `tooManyRecipients` — RFC 8621 §7.5).
    #[serde(rename = "maxRecipients", skip_serializing_if = "Option::is_none")]
    pub max_recipients: Option<u64>,
    /// Invalid recipient addresses (for `invalidRecipients` — RFC 8621 §7.5).
    #[serde(rename = "invalidRecipients", skip_serializing_if = "Option::is_none")]
    pub invalid_recipients: Option<Vec<String>>,
    /// Missing blob IDs (for `blobNotFound` — RFC 8621 §5.5).
    #[serde(rename = "notFound", skip_serializing_if = "Option::is_none")]
    pub not_found: Option<Vec<Id>>,
    /// Maximum message size in octets (for `tooLarge` on EmailSubmission — RFC 8621 §7.5).
    #[serde(rename = "maxSize", skip_serializing_if = "Option::is_none")]
    pub max_size: Option<u64>,
    /// Catch-all for extension-defined SetError fields not covered by
    /// the typed members above.
    ///
    /// JMAP extensions sometimes ship error variants whose wire shape
    /// includes additional structured fields beyond the RFC 8620 §5.3
    /// base set — e.g. JMAP Chat's slow-mode `rateLimit` SetError carries a
    /// `serverRetryAfter` UTCDate telling the client when it may
    /// retry, and `mdnAlreadySent` (RFC 8621 §7.7) is a typed
    /// extension error variant. This map preserves any such field
    /// across serialize / deserialize round-trip.
    ///
    /// Use [`SetError::with_extra`] to populate from handler code:
    ///
    /// ```ignore
    /// SetError::new(SetErrorType::RateLimit)
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
    /// Keys that the typed fields above serialize to (`type`,
    /// `description`, `properties`, ...) MUST NOT appear in this map:
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
    ///
    /// Takes anything that converts into [`SetErrorType`]: a typed variant,
    /// or an extension wire string such as `"addressBookHasContents"`. A
    /// string naming a known variant builds that variant
    /// (`SetError::new("forbidden")` equals
    /// `SetError::new(SetErrorType::Forbidden)`).
    pub fn new(error_type: impl Into<SetErrorType>) -> Self {
        Self {
            error_type: error_type.into(),
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
    /// to the `Display` output of `jmap_server::JmapBackend::Error`
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
    /// Wrap downstream errors with `jmap_server::server_fail_from_backend`
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
    /// `jmap_server::JmapBackend::Error` (bd:JMAP-sc1b.100) and the matching
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
    pub fn with_existing_id(mut self, id: Id) -> Self {
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
    pub fn with_not_found(mut self, ids: Vec<Id>) -> Self {
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
    /// builder covers — for example JMAP Chat's slow-mode `rateLimit`
    /// SetError carries a `serverRetryAfter` UTCDate:
    ///
    /// ```ignore
    /// SetError::new(SetErrorType::RateLimit)
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
    /// one of the typed-field wire names panics via
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

    /// Validate that [`Self::extra`] does not contain any typed-field
    /// wire name (bd:JMAP-jfia.17).
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
/// contains a key that collides with a typed-field wire-name
/// (bd:JMAP-jfia.17).
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
/// Crate-private (bd:JMAP-jfia.18) so adding a typed field to
/// [`SetError`] stays additive: consumers rely on the `with_extra` /
/// `validate_extras` checks, not on the list's contents.
pub(crate) const RESERVED_SET_ERROR_WIRE_NAMES: &[&str] = &[
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

/// Build the RFC 8620 §5.3 wire object for a [`SetError`] without going
/// through `serde_json::to_value`, so `/set` handlers can fill
/// `notCreated` / `notUpdated` / `notDestroyed` with no fallible step
/// (bd:JMAP-ic0j.38).
///
/// The result equals the derived `Serialize` output: absent optional
/// fields are omitted and each [`SetError::extra`] entry is merged at the
/// top level.
impl From<&SetError> for serde_json::Value {
    fn from(e: &SetError) -> Self {
        fn strings(v: &[impl AsRef<str>]) -> serde_json::Value {
            serde_json::Value::Array(
                v.iter()
                    .map(|s| serde_json::Value::String(s.as_ref().to_owned()))
                    .collect(),
            )
        }
        let mut m = serde_json::Map::new();
        m.insert("type".into(), e.error_type.as_str().into());
        if let Some(d) = &e.description {
            m.insert("description".into(), d.as_str().into());
        }
        if let Some(p) = &e.properties {
            m.insert("properties".into(), strings(p));
        }
        if let Some(id) = &e.existing_id {
            m.insert("existingId".into(), id.as_ref().into());
        }
        if let Some(n) = e.max_recipients {
            m.insert("maxRecipients".into(), n.into());
        }
        if let Some(r) = &e.invalid_recipients {
            m.insert("invalidRecipients".into(), strings(r));
        }
        if let Some(ids) = &e.not_found {
            m.insert("notFound".into(), strings(ids));
        }
        if let Some(n) = e.max_size {
            m.insert("maxSize".into(), n.into());
        }
        m.extend(e.extra.iter().map(|(k, v)| (k.clone(), v.clone())));
        serde_json::Value::Object(m)
    }
}

impl From<SetError> for serde_json::Value {
    fn from(e: SetError) -> Self {
        Self::from(&e)
    }
}

/// The machine-readable type for a [`SetError`] (RFC 8620 §5.3 and RFC 8621).
///
/// # Variant policy
///
/// The variant set below carries:
///
/// - The 10 RFC 8620 §5.3 and §5.4 base error types
///   (`Forbidden`, `OverQuota`, `TooLarge`, `RateLimit`, `NotFound`,
///   `InvalidPatch`, `WillDestroy`, `InvalidProperties`, `Singleton`,
///   `AlreadyExists`).
/// - 13 RFC 8621 mail-specific error types
///   (`MailboxHasChild`, `MailboxHasEmail`, `TooManyKeywords`,
///   `TooManyMailboxes`, `BlobNotFound`, `ForbiddenFrom`,
///   `InvalidEmail`, `TooManyRecipients`, `NoRecipients`,
///   `InvalidRecipients`, `ForbiddenMailFrom`, `ForbiddenToSend`,
///   `CannotUnsend`). They stay typed here so mail servers and mail
///   clients share one typed match (bd:JMAP-wlip.35).
/// - [`Self::Other`] for every other wire string.
///
/// **New extension errors MUST use [`Self::Other`]**, built with
/// `SetErrorType::from("rateLimited")`, `SetError::new("invalidSieve")`
/// and so on. Do not add further extension-specific variants here.
///
/// `From<&str>` and `Deserialize` share one mapping: a known wire string
/// always builds its typed variant, never `Other` (bd:JMAP-wlip.22).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
    /// RFC 8620 §5.4 — an object with the same unique key already exists.
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
    Other(String),
}

crate::impl_string_enum!(
    SetErrorType,
    "a JMAP SetError type string",
    "forbidden" => Forbidden,
    "overQuota" => OverQuota,
    "tooLarge" => TooLarge,
    "rateLimit" => RateLimit,
    "notFound" => NotFound,
    "invalidPatch" => InvalidPatch,
    "willDestroy" => WillDestroy,
    "invalidProperties" => InvalidProperties,
    "singleton" => Singleton,
    "alreadyExists" => AlreadyExists,
    "mailboxHasChild" => MailboxHasChild,
    "mailboxHasEmail" => MailboxHasEmail,
    "tooManyKeywords" => TooManyKeywords,
    "tooManyMailboxes" => TooManyMailboxes,
    "blobNotFound" => BlobNotFound,
    "forbiddenFrom" => ForbiddenFrom,
    "invalidEmail" => InvalidEmail,
    "tooManyRecipients" => TooManyRecipients,
    "noRecipients" => NoRecipients,
    "invalidRecipients" => InvalidRecipients,
    "forbiddenMailFrom" => ForbiddenMailFrom,
    "forbiddenToSend" => ForbiddenToSend,
    "cannotUnsend" => CannotUnsend,
);

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle (bd:JMAP-wlip.22): `SetErrorType::from("forbidden")` MUST
    /// return `SetErrorType::Forbidden`, not `Other("forbidden")`. The
    /// asymmetry where `from("forbidden") != Forbidden` was a real
    /// foot-gun: handler code intending to emit the typed variant via
    /// a string produced an `Other` that was wire-identical
    /// but PartialEq-distinct, breaking test assertions that compared
    /// the deserialised round-trip against the typed expected value.
    ///
    /// Test vector: every known typed variant name canonicalises to its
    /// typed variant; an unknown name stays Other. The wire-name list
    /// is hand-built from the same RFC source as the round-trip test
    /// `set_error_type_all_known_variants_round_trip`.
    #[test]
    fn from_str_canonicalises_known_wire_names_to_typed_variants() {
        // Spot-check a representative subset across the 23 known names.
        // The exhaustive round-trip is exercised by
        // set_error_type_all_known_variants_round_trip; this test focuses
        // on the from() → typed-variant direction the bead was about.
        let cases: &[(&str, SetErrorType)] = &[
            ("forbidden", SetErrorType::Forbidden),
            ("overQuota", SetErrorType::OverQuota),
            ("invalidPatch", SetErrorType::InvalidPatch),
            ("mailboxHasChild", SetErrorType::MailboxHasChild),
            ("tooManyRecipients", SetErrorType::TooManyRecipients),
            ("cannotUnsend", SetErrorType::CannotUnsend),
        ];
        for (name, expected) in cases {
            let from_str = SetErrorType::from(*name);
            assert_eq!(
                &from_str, expected,
                "from({name:?}) must canonicalise to the typed variant, not Other"
            );
            assert!(
                !matches!(from_str, SetErrorType::Other(_)),
                "from({name:?}) must NOT remain Other — known wire-name asymmetry"
            );
        }

        // Unknown names stay Other — extension crates depend on this.
        let unknown = SetErrorType::from("mdnAlreadySent");
        assert!(
            matches!(unknown, SetErrorType::Other(ref s) if s == "mdnAlreadySent"),
            "from('mdnAlreadySent') must remain Other (not a known wire-name)"
        );
    }

    /// Oracle: SetErrorType::Other("mdnAlreadySent") must serialize as the bare
    /// string "mdnAlreadySent" and deserialize back to Other("mdnAlreadySent").
    /// Extension crates depend on this round-trip to emit domain-specific errors.
    #[test]
    fn set_error_type_other_round_trips_as_bare_string() {
        let original = SetErrorType::from("mdnAlreadySent");
        let serialized = serde_json::to_string(&original).expect("serialize");
        assert_eq!(
            serialized, r#""mdnAlreadySent""#,
            "Other must serialize as bare string"
        );
        let deserialized: SetErrorType = serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(
            deserialized, original,
            "Other must deserialize back to Other"
        );
    }

    /// Oracle (bd:JMAP-dha0): SetError gains an `extra` map that captures
    /// extension-defined fields not covered by the typed `with_*` builders.
    /// A handler that emits `rateLimited` with `serverRetryAfter` must
    /// see the value round-trip through serialize / deserialize.
    #[test]
    fn set_error_extra_field_round_trips() {
        let original = SetError::new(SetErrorType::from("rateLimited"))
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
        let mut err = SetError::new(SetErrorType::from("rateLimited"));
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
        let err = SetError::new(SetErrorType::from("rateLimited")).with_extra(
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
            .with_existing_id(Id::from("eid"))
            .with_max_recipients(10)
            .with_invalid_recipients(["bad@example"])
            .with_not_found(vec![Id::from("nfid")])
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
    /// variant of [`SetErrorType`]. This table-driven
    /// test iterates ALL 23 typed variants and asserts:
    ///
    ///   - Display produces the expected camelCase wire string
    ///   - serde_json::to_string emits the same string
    ///   - serde_json::from_str rebuilds the same variant (NOT an Other)
    ///
    /// A drift between Display and Deserialize (e.g. adding "rateLimit"
    /// to Display but forgetting the Deserialize arm) would fail step 3
    /// at first test run because the wire string would round-trip into
    /// `Other("rateLimit")` instead of `RateLimit`. This is the silent
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
            // Serialize
            let serialized = serde_json::to_string(variant).expect("serialize");
            assert_eq!(
                serialized,
                format!("\"{expected_wire}\""),
                "Serialize for {variant:?} did not produce \"{expected_wire}\""
            );
            // Deserialize back — MUST rebuild the typed variant, NOT Other.
            let deserialized: SetErrorType =
                serde_json::from_str(&serialized).expect("deserialize");
            assert_eq!(
                &deserialized, variant,
                "Deserialize of {expected_wire:?} did not rebuild {variant:?} \
                 (likely fell through to Other — Display and Deserialize \
                 match arms have drifted)"
            );
            // Belt-and-braces: explicitly assert NOT Other.
            assert!(
                !matches!(deserialized, SetErrorType::Other(_)),
                "Deserialize of {expected_wire:?} fell through to Other; \
                 Display has an arm but Deserialize visitor doesn't"
            );
        }
    }

    /// Oracle: RFC 8620 §5.3 — "invalidProperties: The record given is
    /// invalid in some way. ... The SetError object SHOULD also have a
    /// property called "properties" of type String[] that lists all the
    /// properties that were invalid." Hand-written expected wire object;
    /// absent optional fields must not appear.
    #[test]
    fn value_from_set_error_matches_rfc8620_invalid_properties() {
        let err = SetError::new(SetErrorType::InvalidProperties)
            .with_properties(["name", "parentId"])
            .with_description("bad");
        let expected = serde_json::json!({
            "type": "invalidProperties",
            "description": "bad",
            "properties": ["name", "parentId"]
        });
        assert_eq!(serde_json::Value::from(&err), expected);
        assert_eq!(serde_json::Value::from(err), expected);
    }

    /// Oracle: RFC 8621 §7.5 — "tooManyRecipients ... A "maxRecipients"
    /// UnsignedInt property MUST also be present", "invalidRecipients ...
    /// An "invalidRecipients" property MUST also be present ... a list of
    /// the invalid addresses", "tooLarge ... A "maxSize" UnsignedInt
    /// property MUST be present"; RFC 8620 §5.4 "alreadyExists ... an
    /// "existingId" property"; RFC 8621 §4.6 blobNotFound "notFound"
    /// property. Every typed field plus an extension field (JMAP Chat
    /// `serverRetryAfter`) against a hand-written object.
    #[test]
    fn value_from_set_error_matches_rfc8621_fields_and_extras() {
        let err = SetError::new("rateLimited")
            .with_existing_id(Id::from("obj-7"))
            .with_max_recipients(50)
            .with_invalid_recipients(["bad@", "no@no"])
            .with_not_found(vec![Id::from("blob-1"), Id::from("blob-2")])
            .with_max_size(10_485_760)
            .with_extra(
                "serverRetryAfter",
                serde_json::json!("2025-12-31T23:59:59Z"),
            );
        let expected = serde_json::json!({
            "type": "rateLimited",
            "existingId": "obj-7",
            "maxRecipients": 50,
            "invalidRecipients": ["bad@", "no@no"],
            "notFound": ["blob-1", "blob-2"],
            "maxSize": 10485760,
            "serverRetryAfter": "2025-12-31T23:59:59Z"
        });
        assert_eq!(serde_json::Value::from(&err), expected);
    }

    /// Oracle: the derived `Serialize` impl, an independent path through
    /// serde. The hand-built conversion must agree with it for a bare
    /// error and for one carrying every field.
    #[test]
    fn value_from_set_error_agrees_with_serde_derive() {
        let full = SetError::new(SetErrorType::TooManyRecipients)
            .with_description("too many")
            .with_properties(["to"])
            .with_existing_id(Id::from("e1"))
            .with_max_recipients(3)
            .with_invalid_recipients(["x@"])
            .with_not_found(vec![Id::from("b1")])
            .with_max_size(1)
            .with_extra("x-ext", serde_json::json!({"k": [1, 2]}));
        for err in [SetError::new(SetErrorType::Forbidden), full] {
            assert_eq!(
                serde_json::Value::from(&err),
                serde_json::to_value(&err).expect("serialize"),
                "conversion drifted from derive(Serialize) for {err:?}"
            );
        }
    }
}
