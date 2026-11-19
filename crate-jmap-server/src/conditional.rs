//! draft-ietf-jmap-conditional-02 support: the backend hooks
//! ([`ConditionalBackend`], [`SetScope`]) and the opt-in registration
//! ([`SetSupport`], [`Dispatcher::attach_set_support`]).
//!
//! Conditional support is opt-in per Foo/set method (docs/design/
//! DECISIONS.md decisions 1 and 17): an extension crate's
//! `register_<x>_conditional` attaches a [`SetSupport`] to each of its
//! registered `/set` methods, after its unchanged `register_<x>_handlers`.
//! A deployment that never calls it sees no API or trait-bound change.
//! It advertises `urn:ietf:params:jmap:conditional` only when
//! [`Dispatcher::conditional_ready`] is true, because the capability
//! covers "every Foo/set" the server offers (§2.1).
//!
//! The dispatcher evaluates `ifUnchangedBy` and applies `atomic` itself,
//! around the registered handler, inside a [`SetScope`] the backend opens
//! (docs/design/conditional.md §5.2). The method's backend calls run
//! while [`current_set_scope`] names that scope.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use jmap_types::conditional::{AtomicFailure, JMAP_CONDITIONAL_URI};
use jmap_types::{Id, JmapError, SetError, SetObject};
use serde_json::{Map, Value, json};

use crate::backend::JmapBackend;
use crate::helpers::decode_pointer_token;
use crate::{
    CallContext, Dispatcher, HandlerFuture, HandlerOutput, JmapHandler, check_patch_paths,
    server_fail_from_backend,
};

// ---- scope ids -------------------------------------------------------------

/// Identifies one Foo/set method call's [`SetScope`].
///
/// Unique within the process. The dispatcher picks one per conditional
/// method call, passes it in [`SetScopeRequest::scope`] and runs the method
/// under it, so a backend can tell which of its calls belong to which open
/// scope ([`current_set_scope`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SetScopeId(u64);

impl SetScopeId {
    /// A scope id not handed out before in this process.
    pub fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

impl fmt::Display for SetScopeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "set-scope-{}", self.0)
    }
}

tokio::task_local! {
    /// The scope of the conditional Foo/set method running on this task.
    pub(crate) static SET_SCOPE: SetScopeId;
}

/// The [`SetScope`] of the conditional Foo/set method running on this task,
/// or `None` outside one.
///
/// Every backend call made while this is `Some(scope)` belongs to that
/// scope: its reads see the scope's writes and its writes commit or roll
/// back with it. Task-locals do not cross `tokio::spawn`, so a backend
/// that spawns work must read the id first and carry it.
pub fn current_set_scope() -> Option<SetScopeId> {
    SET_SCOPE.try_with(|id| *id).ok()
}

/// Run `fut` with [`current_set_scope`] returning `scope`.
pub(crate) async fn in_set_scope<F: Future>(scope: SetScopeId, fut: F) -> F::Output {
    SET_SCOPE.scope(scope, fut).await
}

// ---- the backend hooks -----------------------------------------------------

/// How a Foo/set method's changes must apply.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetMode {
    /// §6.1: no other writer commits to the account between the method's
    /// precondition reads and its writes. Each change still succeeds or
    /// fails on its own.
    Isolated,
    /// §4: all of the method's changes commit, or none do.
    Atomic,
}

/// What [`ConditionalBackend::begin_set`] is asked to open.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct SetScopeRequest<'a> {
    /// The scope's id; [`current_set_scope`] returns it while the method
    /// runs.
    pub scope: SetScopeId,
    /// The method name, e.g. `"Mailbox/set"`.
    pub method: &'a str,
    /// The data type, e.g. `"Mailbox"`.
    pub data_type: &'a str,
    /// The call's `accountId`, if it had a well-formed one.
    pub account_id: Option<&'a Id>,
    /// Isolated or atomic.
    pub mode: SetMode,
    /// The call's arguments, without `ifUnchangedBy` and `atomic`.
    pub args: &'a Value,
}

impl<'a> SetScopeRequest<'a> {
    /// A request for `method` on `data_type`, in a fresh scope.
    pub fn new(
        method: &'a str,
        data_type: &'a str,
        account_id: Option<&'a Id>,
        mode: SetMode,
        args: &'a Value,
    ) -> Self {
        Self {
            scope: SetScopeId::next(),
            method,
            data_type,
            account_id,
            mode,
            args,
        }
    }
}

/// Why [`ConditionalBackend::begin_set`] did not open a scope.
#[non_exhaustive]
#[derive(Debug)]
pub enum BeginSetError<E> {
    /// §4.3: the backend cannot apply this method atomically, "either at
    /// all, or because this particular method is beyond what it can commit
    /// as a unit". The method is rejected with `cannotApplyAtomically`.
    CannotApplyAtomically,
    /// A storage failure; the method fails with a redacted `serverFail`.
    Other(E),
}

/// A change a [`SetScope`] could not commit.
///
/// Ids are the stored ids: a create carries the id the backend minted,
/// which the dispatcher maps back to its creation id.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetOp {
    /// The object created with this id.
    Create(Id),
    /// The update of this object.
    Update(Id),
    /// The destroy of this object.
    Destroy(Id),
    /// The restore of this object (object history).
    Restore(Id),
}

/// One change rejected at commit, with its SetError.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct DeferredFailure {
    /// The change.
    pub op: SetOp,
    /// Why it failed, as it would appear in the Foo/set response.
    pub error: SetError,
}

impl DeferredFailure {
    /// `op` failed with `error`.
    pub fn new(op: SetOp, error: SetError) -> Self {
        Self { op, error }
    }
}

/// Why [`SetScope::commit`] did not commit.
#[non_exhaustive]
#[derive(Debug)]
pub enum CommitError {
    /// An atomic scope's changes broke a constraint checked against the
    /// final state (§4.2). The backend has already rolled back; the method
    /// is rejected with `atomicFailure` naming these changes.
    Rejected(Vec<DeferredFailure>),
    /// A storage failure; the method fails with a redacted `serverFail`.
    Other(Box<dyn std::error::Error + Send + Sync>),
}

impl fmt::Display for CommitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rejected(failures) => {
                write!(f, "commit rejected {} change(s)", failures.len())
            }
            Self::Other(e) => write!(f, "commit failed: {e}"),
        }
    }
}

impl std::error::Error for CommitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Rejected(_) => None,
            Self::Other(e) => Some(&**e),
        }
    }
}

/// The future [`SetScope::commit`] returns.
pub type CommitFuture = Pin<Box<dyn Future<Output = Result<(), CommitError>> + Send>>;

/// The future [`SetScope::rollback`] returns.
pub type RollbackFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

/// An open isolation (and, for [`SetMode::Atomic`], transaction) scope over
/// one Foo/set method call, from [`ConditionalBackend::begin_set`].
///
/// The dispatcher ends it with exactly one of [`commit`](Self::commit) or
/// [`rollback`](Self::rollback). **Dropping a scope without either MUST
/// roll it back**: the dispatcher drops it when the method's task panics
/// or is cancelled.
pub trait SetScope: Send {
    /// Make the scope's writes visible and release it.
    ///
    /// For an atomic scope, check the cross-object constraints
    /// (uniqueness, cycles, single default, children on destroy, quotas)
    /// against the final state; on a violation roll back and return
    /// [`CommitError::Rejected`]. Run deferred external effects (mail send,
    /// federation, push `StateChange`) only once committed.
    fn commit(self: Box<Self>) -> CommitFuture;

    /// Discard the scope's writes and release it. The state string and
    /// change log are as before the scope opened (§4.2).
    fn rollback(self: Box<Self>) -> RollbackFuture;
}

/// The conditional-02 backend hooks, required only by the opt-in
/// `register_<x>_conditional` fns (decision 1).
///
/// Like [`PushSubscriptionBackend`](crate::PushSubscriptionBackend), a
/// separate trait over [`JmapBackend`], so a backend that does not opt in
/// is untouched.
pub trait ConditionalBackend: JmapBackend {
    /// Open the scope one Foo/set method call runs in (§4, §6.1).
    ///
    /// Lock, or start a transaction over, everything the method may read
    /// or write in `request.account_id`, across all types: cascades (a
    /// Mailbox destroy's `onDestroyRemoveEmails`, EmailSubmission's
    /// implicit Email/set) run inside it. Every backend call made while
    /// [`current_set_scope`] is `Some(request.scope)` belongs to the scope,
    /// and `get_state` inside it sees the scope's writes.
    ///
    /// - [`SetMode::Isolated`]: no other writer commits to the account
    ///   between the precondition reads and the method's writes (§6.1).
    /// - [`SetMode::Atomic`] additionally: check cross-object constraints
    ///   at [`SetScope::commit`] against the final state, leave the state
    ///   string and change log unchanged on rollback (§4.2), and defer
    ///   irreversible external effects to commit, or refuse with
    ///   [`BeginSetError::CannotApplyAtomically`] (§4.3). Object-history
    ///   restores in the scope are part of the unit.
    ///
    /// A backend MAY refuse large methods (§9.2).
    ///
    /// No default: a scope that does nothing lets a concurrent write land
    /// between check and apply, so the server would report success for a
    /// write §3.2 requires to be `stateMismatch`. A backend that already
    /// serializes every write per account can return
    /// [`serialized_writes_scope`].
    ///
    /// # Errors
    ///
    /// [`BeginSetError::CannotApplyAtomically`] or a storage error.
    fn begin_set(
        &self,
        caller: &Self::CallerCtx,
        request: SetScopeRequest<'_>,
    ) -> impl Future<Output = Result<Box<dyn SetScope>, BeginSetError<Self::Error>>> + Send;

    /// For each of `ids` (objects of `data_type` in `account_id`), the
    /// top-level properties `caller` may not read, beyond what Foo/get
    /// already leaves out or blanks for them (§9.1).
    ///
    /// A precondition on such a property is answered `forbidden` without
    /// comparing its value, so the answer never depends on data the caller
    /// cannot see.
    ///
    /// Default: nothing hidden (a permission hook, fail-open like the
    /// other permission hooks, bd JMAP-bghk). Override it when the
    /// backend's Foo/get redacts a property per caller.
    ///
    /// # Errors
    ///
    /// A storage error; the method fails with a redacted `serverFail`.
    fn hidden_properties(
        &self,
        caller: &Self::CallerCtx,
        account_id: Option<&Id>,
        data_type: &str,
        ids: &[Id],
    ) -> impl Future<Output = Result<BTreeMap<Id, Vec<String>>, Self::Error>> + Send {
        let _ = (caller, account_id, data_type, ids);
        async { Ok(BTreeMap::new()) }
    }
}

/// A [`SetScope`] that does nothing, for a backend that already serializes
/// every write per account.
///
/// **Correct only when the backend applies every write to an account one
/// at a time, so no other writer can commit between this method's
/// precondition reads and its writes** (§6.1, e.g. one lock held for each
/// whole method call). It refuses [`SetMode::Atomic`] with
/// [`BeginSetError::CannotApplyAtomically`], because it cannot undo
/// writes. Test backends use it too.
///
/// # Errors
///
/// [`BeginSetError::CannotApplyAtomically`] for an atomic request.
pub fn serialized_writes_scope<E>(
    request: SetScopeRequest<'_>,
) -> Result<Box<dyn SetScope>, BeginSetError<E>> {
    match request.mode {
        SetMode::Isolated => Ok(Box::new(NoopScope)),
        _ => Err(BeginSetError::CannotApplyAtomically),
    }
}

/// The scope [`serialized_writes_scope`] returns.
struct NoopScope;

impl SetScope for NoopScope {
    fn commit(self: Box<Self>) -> CommitFuture {
        Box::pin(async { Ok(()) })
    }

    fn rollback(self: Box<Self>) -> RollbackFuture {
        Box::pin(async {})
    }
}

// ---- registration ----------------------------------------------------------

/// A boxed, sendable future borrowing for `'a`.
type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// [`ConditionalBackend`] made object-safe, with backend errors already
/// mapped to redacted `serverFail`s.
pub(crate) trait DynConditional<C>: Send + Sync {
    fn begin_set<'a>(
        &'a self,
        caller: &'a C,
        request: SetScopeRequest<'a>,
    ) -> BoxFuture<'a, Result<Box<dyn SetScope>, BeginSetError<JmapError>>>;

    fn hidden_properties<'a>(
        &'a self,
        caller: &'a C,
        account_id: Option<&'a Id>,
        data_type: &'a str,
        ids: &'a [Id],
    ) -> BoxFuture<'a, Result<BTreeMap<Id, Vec<String>>, JmapError>>;
}

struct Adapter<B>(Arc<B>);

impl<B: ConditionalBackend> DynConditional<B::CallerCtx> for Adapter<B> {
    fn begin_set<'a>(
        &'a self,
        caller: &'a B::CallerCtx,
        request: SetScopeRequest<'a>,
    ) -> BoxFuture<'a, Result<Box<dyn SetScope>, BeginSetError<JmapError>>> {
        Box::pin(async move {
            self.0
                .begin_set(caller, request)
                .await
                .map_err(|e| match e {
                    BeginSetError::Other(e) => BeginSetError::Other(server_fail_from_backend(&e)),
                    _ => BeginSetError::CannotApplyAtomically,
                })
        })
    }

    fn hidden_properties<'a>(
        &'a self,
        caller: &'a B::CallerCtx,
        account_id: Option<&'a Id>,
        data_type: &'a str,
        ids: &'a [Id],
    ) -> BoxFuture<'a, Result<BTreeMap<Id, Vec<String>>, JmapError>> {
        Box::pin(async move {
            self.0
                .hidden_properties(caller, account_id, data_type, ids)
                .await
                .map_err(|e| server_fail_from_backend(&e))
        })
    }
}

/// Conditional support for one Foo/set method, attached with
/// [`Dispatcher::attach_set_support`].
///
/// Carries the data type (the dispatcher reads preconditions through the
/// registered `"<type>/get"`), its property names (Foo/get alone does not
/// reject unknown names, so the dispatcher checks pointers against them)
/// and the backend that opens the method's [`SetScope`].
pub struct SetSupport<C> {
    data_type: &'static str,
    properties: [&'static [&'static str]; 2],
    backend: Arc<dyn DynConditional<C>>,
    /// The evaluator, instantiated where `C` is known to be `Sync` (the
    /// dispatcher's own `CallerCtx` bound does not say so).
    run: RunFn<C>,
}

/// [`run`] for one caller type.
type RunFn<C> = fn(Prepared<C>, Arc<dyn JmapHandler<C>>, CallContext<C>) -> HandlerFuture;

impl<C> SetSupport<C> {
    /// Support for `O`'s `/set`, with scopes from `backend`.
    pub fn new<O, B>(backend: Arc<B>) -> Self
    where
        O: SetObject,
        B: ConditionalBackend<CallerCtx = C>,
        // Always true for a backend's `CallerCtx`; stated so the evaluator
        // can be instantiated here.
        C: Clone + Send + Sync + 'static,
    {
        Self {
            data_type: O::TYPE_NAME,
            properties: [O::SERVER_SET, O::CLIENT_SETTABLE],
            backend: Arc::new(Adapter(backend)),
            run: |prepared, handler, cx| Box::pin(run(prepared, handler, cx)),
        }
    }

    /// The data type, e.g. `"Mailbox"`.
    pub fn data_type(&self) -> &'static str {
        self.data_type
    }

    /// Whether `name` is one of the type's properties.
    pub(crate) fn is_property(&self, name: &str) -> bool {
        self.properties.iter().any(|list| list.contains(&name))
    }

    pub(crate) fn backend(&self) -> &Arc<dyn DynConditional<C>> {
        &self.backend
    }
}

impl<C> Clone for SetSupport<C> {
    fn clone(&self) -> Self {
        Self {
            data_type: self.data_type,
            properties: self.properties,
            backend: Arc::clone(&self.backend),
            run: self.run,
        }
    }
}

impl<C> fmt::Debug for SetSupport<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SetSupport")
            .field("data_type", &self.data_type)
            .finish_non_exhaustive()
    }
}

/// Why [`Dispatcher::attach_set_support`] refused.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistrationError {
    /// No handler is registered under the method name.
    NotRegistered(String),
    /// The method name does not end in `/set`.
    NotASetMethod(String),
    /// The support is for another data type than the method's.
    DataTypeMismatch {
        /// The method name.
        method: String,
        /// The support's data type.
        data_type: &'static str,
    },
}

impl fmt::Display for RegistrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRegistered(m) => write!(f, "no handler is registered for {m:?}"),
            Self::NotASetMethod(m) => write!(f, "{m:?} is not a Foo/set method"),
            Self::DataTypeMismatch { method, data_type } => {
                write!(f, "support for {data_type} attached to {method:?}")
            }
        }
    }
}

impl std::error::Error for RegistrationError {}

impl<C: Clone + Send + 'static> Dispatcher<C> {
    /// Attach conditional support to the registered Foo/set `method`.
    ///
    /// Replaces support attached before. A later [`Dispatcher::register`]
    /// of `method` drops it, so an overriding handler must opt in again;
    /// until then the method shows in [`conditional_gaps`](Self::conditional_gaps).
    ///
    /// # Errors
    ///
    /// [`RegistrationError`] when `method` is not registered, does not end
    /// in `/set`, or is not `"<support's data type>/set"`.
    pub fn attach_set_support(
        &mut self,
        method: &str,
        support: SetSupport<C>,
    ) -> Result<(), RegistrationError> {
        let Some(data_type) = method.strip_suffix("/set") else {
            return Err(RegistrationError::NotASetMethod(method.to_owned()));
        };
        if !self.handlers.contains_key(method) {
            return Err(RegistrationError::NotRegistered(method.to_owned()));
        }
        if data_type != support.data_type {
            return Err(RegistrationError::DataTypeMismatch {
                method: method.to_owned(),
                data_type: support.data_type,
            });
        }
        self.set_support.insert(method.to_owned(), support);
        Ok(())
    }

    /// The registered Foo/set methods conditional-02 cannot serve yet: those
    /// without attached [`SetSupport`], or whose `"<type>/get"` is not
    /// registered (the dispatcher reads preconditions through it). Sorted.
    pub fn conditional_gaps(&self) -> Vec<String> {
        let mut gaps: Vec<String> = self
            .handlers
            .keys()
            .filter_map(|method| {
                let data_type = method.strip_suffix("/set")?;
                let served = self.set_support.contains_key(method)
                    && self.handlers.contains_key(&format!("{data_type}/get"));
                (!served).then(|| method.clone())
            })
            .collect();
        gaps.sort();
        gaps
    }

    /// Whether conditional-02 can be advertised: some support is attached
    /// and [`conditional_gaps`](Self::conditional_gaps) is empty.
    ///
    /// A server MUST NOT put `urn:ietf:params:jmap:conditional` in its
    /// Session unless this is true: the capability covers "every Foo/set"
    /// (§2.1).
    pub fn conditional_ready(&self) -> bool {
        !self.set_support.is_empty() && self.conditional_gaps().is_empty()
    }

    /// The support attached to `method`, if any.
    #[cfg(test)]
    pub(crate) fn set_support(&self, method: &str) -> Option<&SetSupport<C>> {
        self.set_support.get(method)
    }
}

// ---- evaluation (docs/design/conditional.md §5.2) -------------------------

/// The §3 arguments of one Foo/set call, checked.
pub(crate) struct ConditionalArgs {
    /// Key as sent (an id or `#creationId`) → precondition; `None` is the
    /// JSON `null` "does not exist" precondition.
    if_unchanged_by: BTreeMap<String, Precondition>,
    atomic: bool,
}

/// One `ifUnchangedBy` value: a PatchObject, or `None` for JSON `null`.
type Precondition = Option<Map<String, Value>>;

/// An `ifUnchangedBy` entry: its key as sent, the id it resolves to (none
/// for a `#creationId` with no creation), and its precondition.
type Resolved<'a> = (&'a String, Option<Id>, &'a Precondition);

/// What the evaluator needs from the dispatcher, gathered before the
/// method's task is spawned (the dispatcher is not `'static`).
pub(crate) struct Prepared<C> {
    args: ConditionalArgs,
    support: SetSupport<C>,
    /// The registered `"<type>/get"`.
    get: Option<Arc<dyn JmapHandler<C>>>,
    /// The dispatcher guard's verdict on `"<type>/get"` for this caller.
    get_guard: Result<(), JmapError>,
}

impl<C> Prepared<C> {
    /// Run `handler` for `cx` under the conditional evaluator.
    pub(crate) fn run(self, handler: Arc<dyn JmapHandler<C>>, cx: CallContext<C>) -> HandlerFuture {
        (self.support.run)(self, handler, cx)
    }
}

fn invalid(description: impl Into<String>) -> JmapError {
    JmapError::invalid_arguments(description)
}

/// Decision 2: without the URI in `using`, `atomic` must be absent, `null`
/// or `false` and `ifUnchangedBy` absent, `null` or `{}`; those defaults are
/// removed so the handler sees what it saw before conditional-02.
fn strip_without_uri(args: &mut Map<String, Value>) -> Result<(), JmapError> {
    let atomic_ok = matches!(
        args.remove("atomic"),
        None | Some(Value::Null | Value::Bool(false))
    );
    let preconditions_ok = match args.remove("ifUnchangedBy") {
        None | Some(Value::Null) => true,
        Some(Value::Object(m)) => m.is_empty(),
        Some(_) => false,
    };
    if atomic_ok && preconditions_ok {
        Ok(())
    } else {
        Err(invalid(format!(
            "ifUnchangedBy/atomic require {JMAP_CONDITIONAL_URI} in using"
        )))
    }
}

/// Remove and check `ifUnchangedBy` and `atomic` (§3; `null` is the
/// default). `None` when both are at their defaults.
fn take_args(args: &mut Map<String, Value>) -> Result<Option<ConditionalArgs>, JmapError> {
    let atomic = match args.remove("atomic") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => b,
        Some(_) => return Err(invalid("atomic must be a boolean")),
    };
    let mut if_unchanged_by = BTreeMap::new();
    match args.remove("ifUnchangedBy") {
        None | Some(Value::Null) => {}
        Some(Value::Object(entries)) => {
            for (key, value) in entries {
                let id = key.strip_prefix('#').unwrap_or(&key);
                if let Err(e) = Id::new_validated(id) {
                    return Err(invalid(format!(
                        "ifUnchangedBy key {key:?} is not an id: {e}"
                    )));
                }
                let precondition = match value {
                    Value::Null => None,
                    Value::Object(patch) => {
                        check_patch_paths(&patch)
                            .map_err(|e| invalid(format!("ifUnchangedBy/{key}: {e}")))?;
                        Some(patch)
                    }
                    _ => {
                        return Err(invalid(format!(
                            "ifUnchangedBy/{key} must be a PatchObject or null"
                        )));
                    }
                };
                if_unchanged_by.insert(key, precondition);
            }
        }
        Some(_) => return Err(invalid("ifUnchangedBy must be an object or null")),
    }
    if if_unchanged_by.is_empty() && !atomic {
        return Ok(None);
    }
    Ok(Some(ConditionalArgs {
        if_unchanged_by,
        atomic,
    }))
}

impl<C: Clone + Send + 'static> Dispatcher<C> {
    /// Steps 1-3 of §5.2 for a Foo/set call: the decision-2 check, the
    /// argument checks, and the lookups the evaluator needs. `Ok(None)`:
    /// run the handler as usual (the defaults have been removed from
    /// `args`).
    pub(crate) fn prepare_conditional(
        &self,
        method: &str,
        args: &mut Value,
        using: &[String],
        caller: &C,
    ) -> Result<Option<Prepared<C>>, JmapError> {
        let Value::Object(map) = args else {
            return Ok(None);
        };
        if !using.iter().any(|u| u == JMAP_CONDITIONAL_URI) {
            strip_without_uri(map)?;
            return Ok(None);
        }
        let Some(conditional) = take_args(map)? else {
            return Ok(None);
        };
        let unsupported = || invalid(format!("conditional {method} is not supported"));
        let Some(support) = self.set_support.get(method).cloned() else {
            return Err(unsupported());
        };
        let get_method = format!("{}/get", support.data_type());
        let get = self.handlers.get(&get_method).map(Arc::clone);
        if get.is_none() && !conditional.if_unchanged_by.is_empty() {
            return Err(unsupported());
        }
        let get_guard = self
            .guard
            .as_ref()
            .map_or(Ok(()), |guard| guard(&get_method, caller));
        Ok(Some(Prepared {
            args: conditional,
            support,
            get,
            get_guard,
        }))
    }
}

/// Steps 4-11 of §5.2: open the scope, check the preconditions, run the
/// handler, and commit or roll back.
async fn run<C: Clone + Send + Sync + 'static>(
    prepared: Prepared<C>,
    handler: Arc<dyn JmapHandler<C>>,
    cx: CallContext<C>,
) -> Result<HandlerOutput, JmapError> {
    let mode = if prepared.args.atomic {
        SetMode::Atomic
    } else {
        SetMode::Isolated
    };
    let account_id = cx
        .args
        .get("accountId")
        .and_then(Value::as_str)
        .and_then(|s| Id::new_validated(s).ok());
    let scope_id = SetScopeId::next();
    let request = SetScopeRequest {
        scope: scope_id,
        method: &cx.method,
        data_type: prepared.support.data_type(),
        account_id: account_id.as_ref(),
        mode,
        args: &cx.args,
    };
    let scope = prepared
        .support
        .backend()
        .begin_set(&cx.caller, request)
        .await
        .map_err(|e| match e {
            BeginSetError::Other(e) => e,
            _ => JmapError::cannot_apply_atomically(),
        })?;
    // The scope is owned by this task: a panic drops it, which rolls back.
    in_set_scope(scope_id, scoped(scope, prepared, account_id, handler, cx)).await
}

async fn scoped<C: Clone + Send + Sync + 'static>(
    scope: Box<dyn SetScope>,
    prepared: Prepared<C>,
    account_id: Option<Id>,
    handler: Arc<dyn JmapHandler<C>>,
    cx: CallContext<C>,
) -> Result<HandlerOutput, JmapError> {
    if let Err(e) = check_preconditions(&prepared, account_id.as_ref(), &cx).await {
        scope.rollback().await;
        return Err(e);
    }
    let atomic = prepared.args.atomic;
    let out = match handler.call(cx.with_atomic(atomic)).await {
        Ok(out) => out,
        Err(e) => {
            scope.rollback().await;
            return Err(e);
        }
    };
    if atomic && let Some(err) = set_errors_in(&out.primary) {
        // §4.2: one failing change rejects the method; the others were not
        // committed, so `extra_invocations` (implicit calls) go too.
        scope.rollback().await;
        return Err(err);
    }
    match scope.commit().await {
        Ok(()) => Ok(out),
        Err(CommitError::Rejected(failures)) if atomic => {
            Err(rejected_at_commit(failures, &out.primary))
        }
        Err(e) => Err(server_fail_from_backend(&e)),
    }
}

/// Steps 5-8: read the objects through `"<type>/get"` and evaluate every
/// precondition (§3.1, §9.1).
async fn check_preconditions<C: Clone + Send + Sync + 'static>(
    prepared: &Prepared<C>,
    account_id: Option<&Id>,
    cx: &CallContext<C>,
) -> Result<(), JmapError> {
    let entries = &prepared.args.if_unchanged_by;
    if entries.is_empty() {
        return Ok(());
    }
    let data_type = prepared.support.data_type();
    // §9.1: "the same permissions to read ... as ... Foo/get".
    if prepared.get_guard.is_err() {
        let mut e = JmapError::forbidden();
        e.description = Some(format!("reading {data_type} objects is not permitted"));
        return Err(e);
    }
    let Some(get) = &prepared.get else {
        return Err(invalid(format!(
            "conditional {} is not supported",
            cx.method
        )));
    };

    // Rule 2: a `#creationId` key stands for the id that creation
    // produced; with no such creation the object does not exist.
    let resolved: Vec<Resolved<'_>> = entries
        .iter()
        .map(|(key, pre)| {
            let id = match key.strip_prefix('#') {
                Some(cid) => cx.created_ids.get(&Id::from(cid)).cloned(),
                None => Some(Id::from(key.as_str())),
            };
            (key, id, pre)
        })
        .collect();
    let ids: BTreeSet<&Id> = resolved
        .iter()
        .filter_map(|(_, id, _)| id.as_ref())
        .collect();
    let mut names = BTreeSet::from(["id".to_owned()]);
    for (_, _, pre) in &resolved {
        for pointer in pre.iter().flat_map(Map::keys) {
            names.insert(top_level(pointer));
        }
    }
    let mut get_args = json!({ "ids": ids, "properties": names });
    if let Some(account) = cx.args.get("accountId") {
        get_args["accountId"] = account.clone();
    }
    let got = get
        .call(
            CallContext::new(
                format!("{data_type}/get"),
                cx.call_id.clone(),
                get_args,
                cx.caller.clone(),
            )
            .with_created_ids(cx.created_ids.clone())
            .with_using(cx.using.clone()),
        )
        .await?;

    // Rule 5: ifInState first. A /get without `state` (PushSubscription,
    // Blob) leaves the argument to the handler.
    if let (Some(want), Some(have)) = (
        cx.args.get("ifInState").and_then(Value::as_str),
        got.primary.get("state").and_then(Value::as_str),
    ) && want != have
    {
        return Err(JmapError::state_mismatch());
    }

    let list: BTreeMap<&str, &Map<String, Value>> = got
        .primary
        .get("list")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|o| {
            let o = o.as_object()?;
            Some((o.get("id")?.as_str()?, o))
        })
        .collect();
    let object = |id: &Option<Id>| id.as_ref().and_then(|id| list.get(id.as_ref()).copied());

    // §9.1 needs the properties hidden from this caller; read them first,
    // because rule 4 counts them as known.
    let visible: Vec<Id> = resolved
        .iter()
        .filter(|(_, id, pre)| pre.is_some() && object(id).is_some())
        .filter_map(|(_, id, _)| id.clone())
        .collect();
    let hidden = if visible.is_empty() {
        BTreeMap::new()
    } else {
        prepared
            .support
            .backend()
            .hidden_properties(&cx.caller, account_id, data_type, &visible)
            .await?
    };

    // Rule 4: every pointer names a property of the type. Foo/get's own
    // objects admit open-ended names (`header:*`, `digest:*`), and so do
    // the names hidden from this caller: an extension property Foo/get
    // leaves out for them (a Task `chatId` the caller may not view) is
    // still a property, which §9.1 answers with `forbidden`.
    let marked = |name: &str| {
        got.unreadable.values().any(|s| s.contains(name))
            || hidden.values().any(|v| v.iter().any(|h| h == name))
    };
    for (key, _, pre) in &resolved {
        for pointer in pre.iter().flat_map(Map::keys) {
            let name = top_level(pointer);
            let known = prepared.support.is_property(&name)
                || list.values().any(|o| o.contains_key(&name))
                || marked(&name);
            if !known {
                return Err(invalid(format!(
                    "ifUnchangedBy/{key}: {name} is not a {data_type} property"
                )));
            }
        }
    }

    // §9.1: readability before any value, so `forbidden` never depends on
    // one. Invisible objects are left to step 8 (stateMismatch).
    for (key, id, pre) in &resolved {
        let (Some(id), Some(patch)) = (id, pre) else {
            continue;
        };
        if object(&Some(id.clone())).is_none() {
            continue;
        }
        for pointer in patch.keys() {
            let name = top_level(pointer);
            let unreadable = got.unreadable.get(id).is_some_and(|s| s.contains(&name))
                || hidden.get(id).is_some_and(|v| v.contains(&name));
            if unreadable {
                let mut e = JmapError::forbidden();
                e.description = Some(format!("ifUnchangedBy/{key}: {name} is not readable"));
                return Err(e);
            }
        }
    }

    // Step 8: values.
    let mut failed: Vec<Option<Id>> = Vec::new();
    for (_, id, pre) in &resolved {
        let holds = match (pre, object(id)) {
            (None, found) => found.is_none(),
            (Some(_), None) => false,
            (Some(patch), Some(o)) => patch.iter().all(|(p, v)| pointer_holds(o, p, v)),
        };
        if !holds {
            failed.push(id.clone());
        }
    }
    if failed.is_empty() {
        return Ok(());
    }
    // `failed` names resolved ids only; an unresolved creation id has
    // none to name, so the list is left out.
    match failed.into_iter().collect::<Option<Vec<Id>>>() {
        Some(ids) => Err(JmapError::state_mismatch_failed(ids)),
        None => Err(JmapError::state_mismatch()),
    }
}

/// The decoded first token of a checked pointer.
fn top_level(pointer: &str) -> String {
    let first = pointer.split('/').next().unwrap_or_default();
    decode_pointer_token(first).map_or_else(|_| first.to_owned(), |t| t.into_owned())
}

/// Whether `object`'s value at `pointer` equals `expected` (§3): every
/// parent must exist and be an object; `null` matches an absent member.
fn pointer_holds(object: &Map<String, Value>, pointer: &str, expected: &Value) -> bool {
    let Ok(tokens) = pointer
        .split('/')
        .map(decode_pointer_token)
        .collect::<Result<Vec<_>, _>>()
    else {
        return false;
    };
    let Some((leaf, parents)) = tokens.split_last() else {
        return false;
    };
    let mut current = object;
    for token in parents {
        match current.get(token.as_ref()) {
            Some(Value::Object(m)) => current = m,
            _ => return false,
        }
    }
    match (expected, current.get(leaf.as_ref())) {
        (Value::Null, None | Some(Value::Null)) => true,
        (expected, Some(actual)) => expected == actual,
        (_, None) => false,
    }
}

/// The keys of a Foo/set response whose entries are failures.
const FAILURE_MAPS: [&str; 4] = ["notCreated", "notUpdated", "notDestroyed", "notRestored"];

/// §4.2: an `atomicFailure` carrying the handler's non-empty failure maps,
/// or `None` when every change succeeded.
fn set_errors_in(primary: &Value) -> Option<JmapError> {
    let mut err = JmapError::atomic_failure(AtomicFailure::default());
    for key in FAILURE_MAPS {
        if let Some(map) = primary.get(key).and_then(Value::as_object)
            && !map.is_empty()
        {
            err.extra.insert(key.to_owned(), Value::Object(map.clone()));
        }
    }
    (!err.extra.is_empty()).then_some(err)
}

/// §4.2: the changes a commit rejected, as an `atomicFailure`. A create is
/// named by its creation id, found in the handler's `created` map.
fn rejected_at_commit(failures: Vec<DeferredFailure>, primary: &Value) -> JmapError {
    let created = primary.get("created").and_then(Value::as_object);
    let creation_id = |id: &Id| {
        created
            .and_then(|m| {
                m.iter()
                    .find(|(_, o)| o.get("id").and_then(Value::as_str) == Some(id.as_ref()))
            })
            .map_or_else(|| id.to_string(), |(cid, _)| cid.clone())
    };
    let mut details = AtomicFailure::default();
    let mut not_restored = BTreeMap::new();
    for DeferredFailure { op, error } in failures {
        match op {
            SetOp::Create(id) => {
                details
                    .not_created
                    .get_or_insert_with(BTreeMap::new)
                    .insert(creation_id(&id), error);
            }
            SetOp::Update(id) => {
                details
                    .not_updated
                    .get_or_insert_with(BTreeMap::new)
                    .insert(id, error);
            }
            SetOp::Destroy(id) => {
                details
                    .not_destroyed
                    .get_or_insert_with(BTreeMap::new)
                    .insert(id, error);
            }
            SetOp::Restore(id) => {
                not_restored.insert(id, error);
            }
        }
    }
    if !not_restored.is_empty() {
        details = details.with_not_restored(not_restored);
    }
    JmapError::atomic_failure(details)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Mutex;

    use jmap_types::{JmapObject, MethodErrorType, State};
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    use super::*;
    use crate::backend::{
        BackendChangesError, BackendQueryError, ChangesParams, ChangesResult, GetObject,
        QueryChangesParams, QueryChangesResult, QueryObject, QueryParams, QueryResult,
    };
    use crate::{CallContext, HandlerFuture, HandlerOutput, JmapHandler};
    use crate::{JmapAccounts, JmapReads};

    #[derive(Debug, Serialize, Deserialize)]
    struct Probe;

    impl JmapObject for Probe {
        const TYPE_NAME: &'static str = "Probe";
        type Property = ();
    }

    impl SetObject for Probe {
        type Patch = Value;
        const SERVER_SET: &'static [&'static str] = &["id"];
        const CLIENT_SETTABLE: &'static [&'static str] = &["name"];
        const IMMUTABLE: &'static [&'static str] = &[];
    }

    #[derive(Debug)]
    struct StubError;

    impl fmt::Display for StubError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("secret storage detail")
        }
    }

    impl std::error::Error for StubError {}

    /// Records what happens to its scopes; `fail` makes `begin_set` fail.
    #[derive(Default)]
    struct Backend {
        log: Arc<Mutex<Vec<String>>>,
        fail: bool,
    }

    impl JmapAccounts for Backend {
        type Error = StubError;
        type CallerCtx = ();

        async fn account_exists(&self, _: &(), _: &Id) -> Result<bool, StubError> {
            unreachable!("not used")
        }
    }

    impl JmapReads for Backend {
        async fn get_objects<O: GetObject + Send + Sync>(
            &self,
            _: &(),
            _: &Id,
            _: Option<&[Id]>,
            _: Option<&[String]>,
        ) -> Result<(Vec<O>, Vec<Id>), StubError> {
            unreachable!("not used")
        }

        async fn get_state<O: JmapObject + Send + Sync>(
            &self,
            _: &(),
            _: &Id,
        ) -> Result<State, StubError> {
            unreachable!("not used")
        }

        async fn get_changes<O: JmapObject + Send + Sync>(
            &self,
            _: &(),
            _: &Id,
            _: ChangesParams<'_>,
        ) -> Result<ChangesResult, BackendChangesError<StubError>> {
            unreachable!("not used")
        }

        async fn query_objects<O: QueryObject + Send + Sync>(
            &self,
            _: &(),
            _: &Id,
            _: QueryParams<'_, O>,
        ) -> Result<QueryResult, BackendQueryError<StubError>> {
            unreachable!("not used")
        }

        async fn query_changes<O: QueryObject + Send + Sync>(
            &self,
            _: &(),
            _: &Id,
            _: QueryChangesParams<'_, O>,
        ) -> Result<QueryChangesResult, BackendChangesError<StubError>> {
            unreachable!("not used")
        }
    }

    impl ConditionalBackend for Backend {
        async fn begin_set(
            &self,
            _: &(),
            request: SetScopeRequest<'_>,
        ) -> Result<Box<dyn SetScope>, BeginSetError<StubError>> {
            if self.fail {
                return Err(BeginSetError::Other(StubError));
            }
            self.log
                .lock()
                .unwrap()
                .push(format!("begin {:?} {}", request.mode, request.method));
            Ok(Box::new(RecordingScope {
                log: Arc::clone(&self.log),
                ended: false,
            }))
        }
    }

    /// A scope that logs how it ended, rolling back on drop as the
    /// [`SetScope`] contract requires.
    struct RecordingScope {
        log: Arc<Mutex<Vec<String>>>,
        ended: bool,
    }

    impl SetScope for RecordingScope {
        fn commit(mut self: Box<Self>) -> CommitFuture {
            self.ended = true;
            self.log.lock().unwrap().push("commit".to_owned());
            Box::pin(async { Ok(()) })
        }

        fn rollback(mut self: Box<Self>) -> RollbackFuture {
            self.ended = true;
            self.log.lock().unwrap().push("rollback".to_owned());
            Box::pin(async {})
        }
    }

    impl Drop for RecordingScope {
        fn drop(&mut self) {
            if !self.ended {
                self.log.lock().unwrap().push("rollback on drop".to_owned());
            }
        }
    }

    /// Answers `{"seen": <args>}`, plus an `unreadable` set.
    struct Echo;

    impl JmapHandler<()> for Echo {
        fn call(&self, cx: CallContext<()>) -> HandlerFuture {
            Box::pin(async move {
                let hidden =
                    BTreeMap::from([(Id::from("p1"), BTreeSet::from(["name".to_owned()]))]);
                Ok(HandlerOutput::new(json!({"seen": cx.args})).with_unreadable(hidden))
            })
        }
    }

    fn support(backend: Backend) -> SetSupport<()> {
        SetSupport::new::<Probe, _>(Arc::new(backend))
    }

    fn dispatcher(methods: &[&str]) -> Dispatcher<()> {
        let mut d = Dispatcher::new();
        for m in methods {
            d.register(*m, Arc::new(Echo));
        }
        d
    }

    #[test]
    fn attach_needs_a_registered_set_of_the_same_type() {
        let mut d = dispatcher(&["Probe/get", "Other/set"]);
        assert_eq!(
            d.attach_set_support("Probe/get", support(Backend::default())),
            Err(RegistrationError::NotASetMethod("Probe/get".to_owned()))
        );
        assert_eq!(
            d.attach_set_support("Probe/set", support(Backend::default())),
            Err(RegistrationError::NotRegistered("Probe/set".to_owned()))
        );
        assert_eq!(
            d.attach_set_support("Other/set", support(Backend::default())),
            Err(RegistrationError::DataTypeMismatch {
                method: "Other/set".to_owned(),
                data_type: "Probe"
            })
        );
        assert!(d.set_support("Other/set").is_none());
    }

    #[test]
    fn gaps_and_readiness() {
        let mut d = dispatcher(&[]);
        assert!(!d.conditional_ready(), "nothing attached");
        d.register("Probe/set", Arc::new(Echo));
        assert_eq!(d.conditional_gaps(), ["Probe/set"]);
        d.attach_set_support("Probe/set", support(Backend::default()))
            .unwrap();
        // Preconditions are read through Probe/get, which is missing.
        assert_eq!(d.conditional_gaps(), ["Probe/set"]);
        assert!(!d.conditional_ready());
        d.register("Probe/get", Arc::new(Echo));
        assert!(d.conditional_gaps().is_empty());
        assert!(d.conditional_ready());
        // Another /set without support is a gap again.
        d.register("Other/set", Arc::new(Echo));
        assert_eq!(d.conditional_gaps(), ["Other/set"]);
        assert!(!d.conditional_ready());
    }

    #[test]
    fn register_drops_support() {
        let mut d = dispatcher(&["Probe/get", "Probe/set"]);
        d.attach_set_support("Probe/set", support(Backend::default()))
            .unwrap();
        // A refused try_register keeps the handler and its support.
        assert!(d.try_register("Probe/set", Arc::new(Echo)).is_err());
        assert!(d.conditional_ready());
        // Replacing the handler drops the support: the new handler must
        // opt in again.
        d.register("Probe/set", Arc::new(Echo));
        assert!(d.set_support("Probe/set").is_none());
        assert_eq!(d.conditional_gaps(), ["Probe/set"]);
        assert!(!d.conditional_ready());
    }

    #[test]
    fn support_knows_its_type_and_properties() {
        let s = support(Backend::default());
        assert_eq!(s.data_type(), "Probe");
        assert!(s.is_property("id"));
        assert!(s.is_property("name"));
        assert!(!s.is_property("bogus"));
        assert!(format!("{s:?}").contains("Probe"));
    }

    #[tokio::test]
    async fn adapter_opens_scopes_and_maps_errors() {
        let backend = Backend::default();
        let log = Arc::clone(&backend.log);
        let s = support(backend);
        let args = json!({});
        let request = SetScopeRequest::new("Probe/set", "Probe", None, SetMode::Atomic, &args);
        let scope = s.backend().begin_set(&(), request).await.unwrap();
        scope.commit().await.unwrap();
        assert_eq!(*log.lock().unwrap(), ["begin Atomic Probe/set", "commit"]);
        let hidden = s
            .backend()
            .hidden_properties(&(), None, "Probe", &[Id::from("p1")])
            .await
            .unwrap();
        assert!(hidden.is_empty(), "default: nothing hidden");

        // A storage error becomes a redacted serverFail.
        let failing = support(Backend {
            fail: true,
            ..Backend::default()
        });
        let request = SetScopeRequest::new("Probe/set", "Probe", None, SetMode::Isolated, &args);
        let Err(BeginSetError::Other(e)) = failing.backend().begin_set(&(), request).await else {
            panic!("expected a storage error");
        };
        assert_eq!(e.error_type, MethodErrorType::ServerFail);
        assert!(!format!("{e:?}").contains("secret"));
    }

    /// The [`SetScope`] contract: a scope dropped without commit or
    /// rollback rolls back.
    #[tokio::test]
    async fn dropped_scope_rolls_back() {
        let backend = Backend::default();
        let log = Arc::clone(&backend.log);
        let args = json!({});
        let request = SetScopeRequest::new("Probe/set", "Probe", None, SetMode::Atomic, &args);
        let scope = backend.begin_set(&(), request).await.unwrap();
        drop(scope);
        let request = SetScopeRequest::new("Probe/set", "Probe", None, SetMode::Atomic, &args);
        backend
            .begin_set(&(), request)
            .await
            .unwrap()
            .rollback()
            .await;
        assert_eq!(
            *log.lock().unwrap(),
            [
                "begin Atomic Probe/set",
                "rollback on drop",
                "begin Atomic Probe/set",
                "rollback"
            ]
        );
    }

    #[tokio::test]
    async fn serialized_writes_scope_refuses_atomic() {
        let args = json!({});
        let isolated = SetScopeRequest::new("Probe/set", "Probe", None, SetMode::Isolated, &args);
        let scope = serialized_writes_scope::<StubError>(isolated).unwrap();
        scope.commit().await.unwrap();
        let isolated = SetScopeRequest::new("Probe/set", "Probe", None, SetMode::Isolated, &args);
        serialized_writes_scope::<StubError>(isolated)
            .unwrap()
            .rollback()
            .await;
        let atomic = SetScopeRequest::new("Probe/set", "Probe", None, SetMode::Atomic, &args);
        assert!(matches!(
            serialized_writes_scope::<StubError>(atomic),
            Err(BeginSetError::CannotApplyAtomically)
        ));
    }

    #[tokio::test]
    async fn current_scope_is_task_local() {
        assert_eq!(current_set_scope(), None);
        let (a, b) = (SetScopeId::next(), SetScopeId::next());
        assert_ne!(a, b);
        let seen = in_set_scope(a, async {
            let inner = current_set_scope();
            // A spawned task does not inherit it.
            let spawned = tokio::spawn(async { current_set_scope() }).await.unwrap();
            (inner, spawned)
        })
        .await;
        assert_eq!(seen, (Some(a), None));
        assert_eq!(current_set_scope(), None);
    }

    #[test]
    fn call_context_atomic_defaults_false() {
        let cx = CallContext::new("Probe/set", "c", json!({}), ());
        assert!(!cx.atomic);
        assert!(cx.with_atomic(true).atomic);
    }

    /// `HandlerOutput::unreadable` never reaches the wire.
    #[tokio::test]
    async fn unreadable_is_not_serialized() {
        let d = dispatcher(&["Probe/get"]);
        let req: jmap_types::JmapRequest = serde_json::from_value(json!({
            "using": ["urn:ietf:params:jmap:core"],
            "methodCalls": [["Probe/get", {"ids": ["p1"]}, "c"]]
        }))
        .unwrap();
        let resp = serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap();
        assert_eq!(
            resp["methodResponses"][0][1],
            json!({"seen": {"ids": ["p1"]}})
        );
        let out = HandlerOutput::new(json!({}));
        assert!(out.unreadable.is_empty());
    }
}
