//! A generic Foo/set call with the draft-ietf-jmap-conditional-02 options
//! ([`SetCall`], [`JmapClient::set_call`]) and the conditional method
//! errors ([`ConditionalError`]) (docs/design/conditional.md §8;
//! docs/design/DECISIONS.md decision 8).

use std::collections::BTreeMap;

use jmap_types::conditional::{
    ATOMIC_FAILURE, AtomicFailure, CANNOT_APPLY_ATOMICALLY, ConditionalSetArgs,
    JMAP_CONDITIONAL_URI,
};
use jmap_types::{Id, JmapError, MethodErrorType, PatchObject};
use serde_json::{Map, Value};

use crate::{ClientError, JmapClient, JmapRequestBuilder, Session, extract_response, to_args};

/// One `Foo/set` call (RFC 8620 §5.3) for any data type, with the
/// conditional-02 options and object-history-03 `restore`.
///
/// Empty fields are left out of the request. Build it with
/// [`SetCall::default`] and set the fields you need; send it with
/// [`JmapClient::set_call`].
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SetCall {
    /// `accountId`.
    pub account_id: Option<Id>,
    /// `ifInState`.
    pub if_in_state: Option<String>,
    /// `create`: creation id → the object to create.
    pub create: BTreeMap<String, Map<String, Value>>,
    /// `update`: id → PatchObject.
    pub update: BTreeMap<Id, PatchObject>,
    /// `destroy`.
    pub destroy: Vec<Id>,
    /// `ifUnchangedBy` and `atomic` (conditional-02 §3, §4). When not
    /// [default](ConditionalSetArgs::is_default), the request adds
    /// `urn:ietf:params:jmap:conditional` to `using`.
    pub conditional: ConditionalSetArgs,
    /// `restore` (draft-ietf-jmap-object-history-03 §5.1): id → the patch
    /// to apply to the restored version, or `None` to restore it as it
    /// was. The caller adds the object-history URI to `using`.
    pub restore: BTreeMap<Id, Option<PatchObject>>,
    /// Any other argument of the type's `/set` (e.g. `onExists`). Typed
    /// fields win over a key of the same name here.
    pub extra: Map<String, Value>,
}

impl SetCall {
    /// The method arguments this call sends.
    ///
    /// # Errors
    ///
    /// [`ClientError::Serialize`] if a value does not serialize.
    pub fn to_arguments(&self) -> Result<Value, ClientError> {
        let mut args = Map::new();
        if let Some(account_id) = &self.account_id {
            args.insert("accountId".to_owned(), to_args(account_id)?);
        }
        if let Some(state) = &self.if_in_state {
            args.insert("ifInState".to_owned(), Value::String(state.clone()));
        }
        if !self.create.is_empty() {
            args.insert("create".to_owned(), to_args(&self.create)?);
        }
        if !self.update.is_empty() {
            args.insert("update".to_owned(), to_args(&self.update)?);
        }
        if !self.destroy.is_empty() {
            args.insert("destroy".to_owned(), to_args(&self.destroy)?);
        }
        if let Value::Object(conditional) = to_args(&self.conditional)? {
            args.extend(conditional);
        }
        if !self.restore.is_empty() {
            args.insert("restore".to_owned(), to_args(&self.restore)?);
        }
        for (k, v) in &self.extra {
            args.entry(k.clone()).or_insert_with(|| v.clone());
        }
        Ok(Value::Object(args))
    }

    /// The request's `using`: core, then `extra_using`, then the
    /// conditional URI when the conditional options are not default; each
    /// URI once.
    pub fn using(&self, extra_using: &[&str]) -> Vec<String> {
        let mut using = vec!["urn:ietf:params:jmap:core".to_owned()];
        let conditional = (!self.conditional.is_default()).then_some(JMAP_CONDITIONAL_URI);
        for uri in extra_using.iter().copied().chain(conditional) {
            if !using.iter().any(|u| u == uri) {
                using.push(uri.to_owned());
            }
        }
        using
    }
}

/// The `using` URI of draft-ietf-jmap-object-history-03, for
/// [`SetOptions::restore`]. `jmap-object-history-types` holds the public
/// constant; this crate does not depend on it.
const OBJECT_HISTORY_URI: &str = "urn:ietf:params:jmap:object-history";

/// The options every typed `Foo/set` binding takes beyond the type's own
/// arguments: the conditional-02 `ifUnchangedBy` and `atomic` (§3, §4) and
/// the object-history-03 `restore` (§5.1).
///
/// New per-set options become new fields here, so the typed bindings
/// need no further signature change. The default adds nothing to the
/// request. Build it with [`SetOptions::default`] and set the fields you
/// need.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SetOptions {
    /// `ifUnchangedBy` and `atomic`. When not
    /// [default](ConditionalSetArgs::is_default), `using` gains
    /// `urn:ietf:params:jmap:conditional`. Check
    /// [`Session::supports_conditional`] first.
    pub conditional: ConditionalSetArgs,
    /// `restore`: id → the patch to apply to the restored version, or
    /// `None` to restore it as it was. When not empty, `using` gains
    /// `urn:ietf:params:jmap:object-history`, and the response's
    /// `restored`/`notRestored` land in its `extra`.
    pub restore: BTreeMap<Id, Option<PatchObject>>,
}

impl SetOptions {
    /// True when nothing is set: the request is the same as without
    /// options.
    pub fn is_default(&self) -> bool {
        self.conditional.is_default() && self.restore.is_empty()
    }

    /// The `/set` request `method` with arguments `args` and these options
    /// merged in, under call id `call_id`.
    ///
    /// `using` is `base_using`, then the conditional URI when
    /// [`conditional`](Self::conditional) is not default, then the
    /// object-history URI when [`restore`](Self::restore) is not empty;
    /// each URI once. Options overwrite a key of the same name already in
    /// `args`.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`] if `args` is not a JSON object.
    /// - [`ClientError::Serialize`] if a value does not serialize.
    pub fn request(
        &self,
        method: &str,
        mut args: Value,
        base_using: &[&str],
        call_id: &str,
    ) -> Result<jmap_types::JmapRequest, ClientError> {
        let Value::Object(map) = &mut args else {
            return Err(ClientError::InvalidArgument(format!(
                "{method}: arguments must be a JSON object"
            )));
        };
        if let Value::Object(conditional) = to_args(&self.conditional)? {
            map.extend(conditional);
        }
        if !self.restore.is_empty() {
            map.insert("restore".to_owned(), to_args(&self.restore)?);
        }
        let conditional = (!self.conditional.is_default()).then_some(JMAP_CONDITIONAL_URI);
        let history = (!self.restore.is_empty()).then_some(OBJECT_HISTORY_URI);
        let mut using: Vec<String> = Vec::new();
        for uri in base_using.iter().copied().chain(conditional).chain(history) {
            if !using.iter().any(|u| u == uri) {
                using.push(uri.to_owned());
            }
        }
        let invocation = jmap_types::Invocation::new(method, args, call_id);
        Ok(jmap_types::JmapRequest::new(using, vec![invocation], None))
    }
}

impl JmapClient {
    /// Send `call` as `method` (e.g. `"FileNode/set"`) to `session`'s API
    /// URL and decode the primary response as `R`.
    ///
    /// `using` is core, `extra_using` (the type's capability, and the
    /// object-history URI when `call.restore` is set), and
    /// `urn:ietf:params:jmap:conditional` when `call.conditional` is not
    /// default. Check [`Session::supports_conditional`] first: a server that
    /// does not advertise it answers `unknownCapability`.
    ///
    /// `R` is usually `jmap_types::SetResponse<PartialObject>`; the
    /// object-history `restored`/`notRestored` land in its `extra`.
    ///
    /// # Errors
    ///
    /// Transport and parse errors, and [`ClientError::MethodError`] for a
    /// method-level error; [`ConditionalError::from_method_error`] reads the
    /// conditional ones.
    pub async fn set_call<R: serde::de::DeserializeOwned>(
        &self,
        session: &Session,
        method: &str,
        extra_using: &[&str],
        call: &SetCall,
    ) -> Result<R, ClientError> {
        let using = call.using(extra_using);
        let using: Vec<&str> = using.iter().map(String::as_str).collect();
        let mut builder = JmapRequestBuilder::new(&using);
        builder.add_call(method, call.to_arguments()?, "0")?;
        let response = self.call_session(session, &builder.build()?).await?;
        extract_response(&response, "0")
    }
}

/// A method-level error of a conditional Foo/set (conditional-02 §3.3,
/// §4.2, §4.3).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum ConditionalError {
    /// `stateMismatch`: a precondition (or `ifInState`) did not hold;
    /// `failed` names the ids whose precondition failed, when the server
    /// says.
    StateMismatch {
        /// The `failed` ids, if present.
        failed: Option<Vec<Id>>,
    },
    /// `atomicFailure`: an atomic Foo/set changed nothing; the details
    /// name the offending changes.
    AtomicFailure(AtomicFailure),
    /// `cannotApplyAtomically`: the server cannot apply this method
    /// atomically. A client that does not know this code treats it as
    /// `serverFail` (§4.3).
    CannotApplyAtomically,
}

impl ConditionalError {
    /// The conditional error `error` is, or `None` for any other error.
    ///
    /// `atomicFailure` and `cannotApplyAtomically` decode to
    /// [`MethodErrorType::Other`] (docs/design/DECISIONS.md decision 7), so
    /// they are matched by name.
    pub fn from_method_error(error: &JmapError) -> Option<Self> {
        match &error.error_type {
            MethodErrorType::StateMismatch => Some(Self::StateMismatch {
                failed: error.failed(),
            }),
            MethodErrorType::Other(name) if name == ATOMIC_FAILURE => {
                Some(Self::AtomicFailure(error.atomic_failure_details()?))
            }
            MethodErrorType::Other(name) if name == CANNOT_APPLY_ATOMICALLY => {
                Some(Self::CannotApplyAtomically)
            }
            _ => None,
        }
    }
}
