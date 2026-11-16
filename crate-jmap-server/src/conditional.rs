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

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use jmap_types::{Id, JmapError, SetError, SetObject};
use serde_json::Value;

use crate::backend::JmapBackend;
use crate::{Dispatcher, server_fail_from_backend};

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
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used by the dispatcher evaluator, bd JMAP-y1yu.3")
)]
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
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "read by the dispatcher evaluator, bd JMAP-y1yu.3")
)]
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

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "read by the dispatcher evaluator, bd JMAP-y1yu.3")
)]
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
}

impl<C> SetSupport<C> {
    /// Support for `O`'s `/set`, with scopes from `backend`.
    pub fn new<O, B>(backend: Arc<B>) -> Self
    where
        O: SetObject,
        B: ConditionalBackend<CallerCtx = C>,
    {
        Self {
            data_type: O::TYPE_NAME,
            properties: [O::SERVER_SET, O::CLIENT_SETTABLE],
            backend: Arc::new(Adapter(backend)),
        }
    }

    /// The data type, e.g. `"Mailbox"`.
    pub fn data_type(&self) -> &'static str {
        self.data_type
    }

    /// Whether `name` is one of the type's properties.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "read by the dispatcher evaluator, bd JMAP-y1yu.3")
    )]
    pub(crate) fn is_property(&self, name: &str) -> bool {
        self.properties.iter().any(|list| list.contains(&name))
    }

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "read by the dispatcher evaluator, bd JMAP-y1yu.3")
    )]
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
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "read by the dispatcher evaluator, bd JMAP-y1yu.3")
    )]
    pub(crate) fn set_support(&self, method: &str) -> Option<&SetSupport<C>> {
        self.set_support.get(method)
    }
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

    impl JmapBackend for Backend {
        type Error = StubError;
        type CallerCtx = ();

        async fn account_exists(&self, _: &(), _: &Id) -> Result<bool, StubError> {
            unreachable!("not used")
        }

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
