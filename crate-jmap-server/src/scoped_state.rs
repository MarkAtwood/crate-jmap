//! [`ScopedState`]: shared in-memory backend state that can open a
//! draft-ietf-jmap-conditional-02 [`SetScope`] (docs/design/conditional.md
//! §7).
//!
//! **Demonstration quality.** This is what the in-repo memory reference
//! backends use to implement [`ConditionalBackend::begin_set`]: one gate
//! per backend (or shared between several), a whole-store clone as the
//! undo log, and final-state checks run at commit. It is correct, but it
//! serializes every scoped method behind one lock and copies the whole
//! store per atomic call. A production backend uses its storage engine's
//! transactions instead.
//!
//! [`ConditionalBackend::begin_set`]: crate::ConditionalBackend::begin_set

use std::fmt;
use std::future::ready;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::OwnedMutexGuard;

use crate::conditional::{
    CommitError, CommitFuture, DeferredFailure, RollbackFuture, SetMode, SetScope, SetScopeId,
    SetScopeRequest, current_set_scope,
};

/// The lock a [`ScopedState`] waits on while another task's [`SetScope`]
/// is open.
///
/// Each [`ScopedState::new`] makes its own gate. Backends that call each
/// other inside one method (a binding crate over two stores, the testjig)
/// share one through [`ScopedState::with_gate`]: with separate gates, two
/// scopes opened in opposite orders deadlock. A task running in the scope
/// that holds the gate passes it, on every state that shares it.
pub struct ScopeGate {
    /// Held by the open scope for its whole life.
    lock: Arc<tokio::sync::Mutex<()>>,
    /// The open scope and its mode, while one is open.
    owner: Mutex<Option<(SetScopeId, SetMode)>>,
}

impl ScopeGate {
    /// A gate with no scope open.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            lock: Arc::new(tokio::sync::Mutex::new(())),
            owner: Mutex::new(None),
        })
    }

    /// The open scope and its mode, if any.
    fn owner(&self) -> Option<(SetScopeId, SetMode)> {
        *self.owner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn set_owner(&self, owner: Option<(SetScopeId, SetMode)>) {
        *self.owner.lock().unwrap_or_else(PoisonError::into_inner) = owner;
    }

    /// The mode of the open scope, when the running task is in it.
    fn owned_by_current(&self) -> Option<SetMode> {
        let current = current_set_scope()?;
        self.owner()
            .and_then(|(id, mode)| (id == current).then_some(mode))
    }
}

impl fmt::Debug for ScopeGate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScopeGate")
            .field("owner", &self.owner())
            .finish_non_exhaustive()
    }
}

/// Access to a [`ScopedState`]'s value, from [`ScopedState::lock`] or
/// [`ScopedState::lock_unscoped`].
///
/// A `std::sync::MutexGuard` underneath: do not hold it across an
/// `.await`.
pub struct ScopedGuard<'a, T>(MutexGuard<'a, T>);

impl<T> Deref for ScopedGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for ScopedGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T: fmt::Debug> fmt::Debug for ScopedGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A final-state check run when an atomic scope commits: given the state
/// before the scope opened and the state it would commit, the changes that
/// break a cross-object constraint (§4.2). Empty means commit.
type Validate<T> = Box<dyn FnOnce(&T, &T) -> Vec<DeferredFailure> + Send>;

/// A memory backend's state behind a mutex, gated by a [`ScopeGate`] so a
/// conditional-02 [`SetScope`] can isolate it and roll it back
/// (docs/design/conditional.md §7).
///
/// **Demonstration quality.** It is correct, but it serializes every scoped
/// method behind one gate and clones the whole value per atomic call. A
/// production backend uses its storage engine's transactions instead.
///
/// Replaces `Arc<Mutex<Inner>>`. Cloning shares the state and the gate.
///
/// - [`lock`](Self::lock) is for async backend methods, reads included: it
///   waits while another task's scope is open, so no caller sees a change
///   an atomic method may still undo. A task in the open scope passes.
/// - [`lock_unscoped`](Self::lock_unscoped) is for sync code and seeding
///   helpers; it never waits on the gate.
/// - [`open`](Self::open) is how `begin_set` opens a scope.
///
/// With no scope ever opened, as in a deployment that does not opt in to
/// conditional-02, the gate is always free and `lock` is a plain mutex
/// lock.
pub struct ScopedState<T> {
    value: Arc<Mutex<T>>,
    gate: Arc<ScopeGate>,
}

impl<T> Clone for ScopedState<T> {
    fn clone(&self) -> Self {
        Self {
            value: Arc::clone(&self.value),
            gate: Arc::clone(&self.gate),
        }
    }
}

impl<T: Default> Default for ScopedState<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: fmt::Debug> fmt::Debug for ScopedState<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScopedState")
            .field("value", &self.value)
            .field("gate", &self.gate)
            .finish()
    }
}

impl<T> ScopedState<T> {
    /// `value`, behind a gate of its own.
    pub fn new(value: T) -> Self {
        Self::with_gate(value, ScopeGate::new())
    }

    /// `value`, behind `gate`, which other states may share.
    pub fn with_gate(value: T, gate: Arc<ScopeGate>) -> Self {
        Self {
            value: Arc::new(Mutex::new(value)),
            gate,
        }
    }

    /// The gate, to share with another state through
    /// [`with_gate`](Self::with_gate).
    pub fn gate(&self) -> &Arc<ScopeGate> {
        &self.gate
    }

    /// Lock the value for an async backend method.
    ///
    /// Waits while a scope opened by another task holds the gate; passes
    /// at once when the running task is in the open scope
    /// ([`current_set_scope`]), or no scope is open. A poisoned mutex is
    /// taken anyway: the value is plain data.
    pub async fn lock(&self) -> ScopedGuard<'_, T> {
        if self.gate.owned_by_current().is_none() {
            // Wait for any open scope to end. Releasing the gate before
            // taking the value is safe: a scope opening next takes the
            // value's mutex to snapshot it, so it waits for this guard.
            drop(self.gate.lock.lock().await);
        }
        self.lock_unscoped()
    }

    /// Lock the value without waiting on the gate, for sync code and
    /// seeding helpers that never run alongside a scoped method.
    pub fn lock_unscoped(&self) -> ScopedGuard<'_, T> {
        ScopedGuard(self.value.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Whether the running task is in an open atomic scope on this
    /// state's gate.
    ///
    /// Per-object methods skip their cross-object checks (unique names,
    /// cycles, single default, quotas) when it is true: the scope's
    /// `validate` runs them against the final state at commit, so a swap
    /// that passes through a conflict still commits (§4.2).
    pub fn deferring(&self) -> bool {
        self.gate.owned_by_current() == Some(SetMode::Atomic)
    }
}

impl<T: Clone + Send + 'static> ScopedState<T> {
    /// Open the [`SetScope`] for `request`, for
    /// [`ConditionalBackend::begin_set`](crate::ConditionalBackend::begin_set).
    ///
    /// Waits for the gate and records `request.scope` as its owner, so
    /// calls made in that scope pass [`lock`](Self::lock) and every other
    /// task waits until the scope ends. If `request.scope` already holds
    /// the gate (a second state sharing it, opened for the same method),
    /// it does not wait.
    ///
    /// [`SetMode::Atomic`] snapshots the value. Rollback, or dropping the
    /// scope, restores the snapshot, so the state string and change log
    /// held in `T` are as before (§4.2). Commit first runs
    /// `validate(&before, &after)`; any failures it returns restore the
    /// snapshot and reject the commit with [`CommitError::Rejected`].
    ///
    /// [`SetMode::Isolated`] takes no snapshot and never calls `validate`:
    /// each change stands on its own (§6.1), and the dispatcher writes
    /// nothing before the handler runs. Ending the scope only releases
    /// the gate.
    pub async fn open<V>(&self, request: SetScopeRequest<'_>, validate: V) -> Box<dyn SetScope>
    where
        V: FnOnce(&T, &T) -> Vec<DeferredFailure> + Send + 'static,
    {
        let held = self.gate.owner().is_some_and(|(id, _)| id == request.scope);
        let permit = if held {
            None
        } else {
            let permit = Arc::clone(&self.gate.lock).lock_owned().await;
            self.gate.set_owner(Some((request.scope, request.mode)));
            Some(permit)
        };
        let atomic = request.mode == SetMode::Atomic;
        let snapshot = atomic.then(|| self.lock_unscoped().clone());
        Box::new(StateScope {
            value: Arc::clone(&self.value),
            gate: Arc::clone(&self.gate),
            permit,
            snapshot,
            validate: atomic.then(|| Box::new(validate) as Validate<T>),
        })
    }
}

/// The [`SetScope`] [`ScopedState::open`] returns.
struct StateScope<T> {
    value: Arc<Mutex<T>>,
    gate: Arc<ScopeGate>,
    /// The gate, when this scope took it (not when another state of the
    /// same scope did).
    permit: Option<OwnedMutexGuard<()>>,
    /// The value before the scope, for an atomic scope not yet ended.
    snapshot: Option<T>,
    validate: Option<Validate<T>>,
}

impl<T> StateScope<T> {
    /// Put the snapshot back, if there is one.
    fn restore(&mut self) {
        if let Some(before) = self.snapshot.take() {
            *self.value.lock().unwrap_or_else(PoisonError::into_inner) = before;
        }
    }
}

impl<T: Send + 'static> SetScope for StateScope<T> {
    fn commit(mut self: Box<Self>) -> CommitFuture {
        let failures = match (self.snapshot.as_ref(), self.validate.take()) {
            (Some(before), Some(validate)) => {
                let after = self.value.lock().unwrap_or_else(PoisonError::into_inner);
                validate(before, &after)
            }
            _ => Vec::new(),
        };
        let result = if failures.is_empty() {
            self.snapshot = None;
            Ok(())
        } else {
            self.restore();
            Err(CommitError::Rejected(failures))
        };
        drop(self);
        Box::pin(ready(result))
    }

    fn rollback(self: Box<Self>) -> RollbackFuture {
        // Drop restores the snapshot and releases the gate.
        drop(self);
        Box::pin(ready(()))
    }
}

impl<T> Drop for StateScope<T> {
    fn drop(&mut self) {
        // A scope not committed rolls back (the SetScope contract), before
        // the gate is released below so no waiter sees the undone writes.
        self.restore();
        if let Some(permit) = self.permit.take() {
            self.gate.set_owner(None);
            drop(permit);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    use jmap_types::{Id, SetError};
    use serde_json::Value;

    use super::*;
    use crate::SetErrorType;
    use crate::conditional::{SetOp, in_set_scope};

    /// A memory store with a state counter and change log, like the
    /// memory backends' `Inner`.
    #[derive(Debug, Clone, Default, PartialEq)]
    struct Store {
        names: BTreeMap<String, String>,
        state: u64,
        log: Vec<String>,
    }

    impl Store {
        fn write(&mut self, id: &str, name: &str) {
            self.names.insert(id.to_owned(), name.to_owned());
            self.state += 1;
            self.log.push(id.to_owned());
        }
    }

    fn seeded() -> ScopedState<Store> {
        let state = ScopedState::<Store>::default();
        state.lock_unscoped().write("a", "one");
        state
    }

    /// `fut`'s output, which must be ready on the first poll: the call
    /// did not wait.
    fn now<F: Future>(fut: F) -> F::Output {
        let mut cx = Context::from_waker(Waker::noop());
        match pin!(fut).poll(&mut cx) {
            Poll::Ready(out) => out,
            Poll::Pending => panic!("waited, but nothing should block"),
        }
    }

    /// Whether `fut` would wait on its first poll.
    fn waits<F: Future>(fut: F) -> bool {
        let mut cx = Context::from_waker(Waker::noop());
        pin!(fut).poll(&mut cx).is_pending()
    }

    fn request(mode: SetMode) -> SetScopeRequest<'static> {
        SetScopeRequest::new("Probe/set", "Probe", None, mode, &Value::Null)
    }

    fn no_checks(_: &Store, _: &Store) -> Vec<DeferredFailure> {
        Vec::new()
    }

    #[test]
    fn lock_never_waits_with_no_scope_open() {
        let state = seeded();
        assert_eq!(now(state.lock()).names["a"], "one");
        // Nor once a scope has come and gone.
        let scope = now(state.open(request(SetMode::Atomic), no_checks));
        now(scope.commit()).unwrap();
        now(state.lock()).write("b", "two");
        assert_eq!(state.lock_unscoped().names.len(), 2);
    }

    #[tokio::test]
    async fn unscoped_read_and_write_wait_for_another_tasks_scope() {
        let state = seeded();
        let req = request(SetMode::Atomic);
        let scope = state.open(req, no_checks).await;
        in_set_scope(req.scope, async { state.lock().await.write("a", "scoped") }).await;

        let reader = state.clone();
        let read = tokio::spawn(async move { reader.lock().await.names["a"].clone() });
        let writer = state.clone();
        let write = tokio::spawn(async move { writer.lock().await.write("b", "outside") });
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert!(!read.is_finished(), "a read must not see the open scope");
        assert!(!write.is_finished(), "a write must wait for the open scope");

        scope.rollback().await;
        // The reader never saw "scoped"; the writer landed after rollback.
        assert_eq!(read.await.unwrap(), "one");
        write.await.unwrap();
        let after = state.lock_unscoped();
        assert_eq!(after.names["a"], "one");
        assert_eq!(after.names["b"], "outside");
        assert_eq!(after.log, ["a", "b"]);
    }

    #[test]
    fn scope_owner_reenters_without_waiting() {
        let state = seeded();
        let req = request(SetMode::Isolated);
        let _scope = now(state.open(req, no_checks));
        let len = now(in_set_scope(req.scope, async {
            state.lock().await.write("b", "two");
            state.lock().await.write("c", "three");
            state.lock().await.names.len()
        }));
        assert_eq!(len, 3);
        // Outside the scope, the same lock waits.
        assert!(waits(state.lock()));
    }

    #[test]
    fn states_sharing_a_gate_do_not_deadlock() {
        let a = seeded();
        let b = ScopedState::with_gate(Store::default(), Arc::clone(a.gate()));
        let req = request(SetMode::Atomic);
        let scope_a = now(a.open(req, no_checks));
        // Opening the second state for the same scope does not wait on
        // the gate the first already holds.
        let scope_b = now(b.open(req, no_checks));
        now(in_set_scope(req.scope, async {
            a.lock().await.write("a", "changed");
            b.lock().await.write("x", "new");
            a.lock().await.write("a", "again");
        }));
        assert!(waits(b.lock()), "b is gated by a's scope");

        // b's scope rolls back b only and keeps the gate held.
        now(scope_b.rollback());
        assert_eq!(*b.lock_unscoped(), Store::default());
        assert!(waits(a.lock()));
        now(scope_a.commit()).unwrap();
        assert_eq!(now(a.lock()).names["a"], "again");
        assert_eq!(*now(b.lock()), Store::default());
    }

    #[test]
    fn rollback_restores_state_and_change_log() {
        let state = seeded();
        let before = state.lock_unscoped().clone();
        let req = request(SetMode::Atomic);
        let scope = now(state.open(req, no_checks));
        now(in_set_scope(req.scope, async {
            let mut s = state.lock().await;
            s.write("a", "renamed");
            s.write("b", "created");
        }));
        assert_eq!(state.lock_unscoped().state, 3);
        now(scope.rollback());
        assert_eq!(*state.lock_unscoped(), before);
        assert_eq!(before.state, 1);
        assert_eq!(before.log, ["a"]);
    }

    #[test]
    fn dropping_a_scope_rolls_back_and_frees_the_gate() {
        let state = seeded();
        let before = state.lock_unscoped().clone();
        let req = request(SetMode::Atomic);
        let scope = now(state.open(req, no_checks));
        now(in_set_scope(req.scope, async {
            state.lock().await.write("b", "two")
        }));
        drop(scope);
        assert_eq!(*now(state.lock()), before);
    }

    #[test]
    fn commit_keeps_changes_validated_against_the_final_state() {
        let state = seeded();
        let req = request(SetMode::Atomic);
        let scope = now(state.open(req, |before: &Store, after: &Store| {
            assert_eq!(before.names["a"], "one");
            assert_eq!(after.names["a"], "two");
            Vec::new()
        }));
        now(in_set_scope(req.scope, async {
            state.lock().await.write("a", "two")
        }));
        now(scope.commit()).unwrap();
        let after = state.lock_unscoped();
        assert_eq!((after.names["a"].as_str(), after.state), ("two", 2));
    }

    #[test]
    fn rejected_commit_restores_and_reports_the_failures() {
        let state = seeded();
        let before = state.lock_unscoped().clone();
        let req = request(SetMode::Atomic);
        // Final-state uniqueness: no two objects share a name.
        let scope = now(state.open(req, |_: &Store, after: &Store| {
            let mut seen = BTreeMap::new();
            after
                .names
                .iter()
                .filter(|(_, name)| seen.insert(name.as_str(), ()).is_some())
                .map(|(id, _)| {
                    DeferredFailure::new(
                        SetOp::Update(Id::new_validated(id).unwrap()),
                        SetError::new(SetErrorType::AlreadyExists),
                    )
                })
                .collect()
        }));
        now(in_set_scope(req.scope, async {
            state.lock().await.write("b", "one")
        }));
        let Err(CommitError::Rejected(failures)) = now(scope.commit()) else {
            panic!("a duplicate name must reject the commit");
        };
        assert_eq!(failures.len(), 1);
        assert_eq!(
            failures[0].op,
            SetOp::Update(Id::new_validated("b").unwrap())
        );
        assert_eq!(
            *now(state.lock()),
            before,
            "rejected: rolled back, gate free"
        );
    }

    #[test]
    fn isolated_scope_keeps_its_writes_and_skips_validate() {
        let state = seeded();
        let req = request(SetMode::Isolated);
        let scope = now(
            state.open(req, |_: &Store, _: &Store| -> Vec<DeferredFailure> {
                panic!("an isolated scope has no final-state check")
            }),
        );
        now(in_set_scope(req.scope, async {
            assert!(!state.deferring());
            state.lock().await.write("b", "two");
        }));
        now(scope.commit()).unwrap();
        assert_eq!(now(state.lock()).names.len(), 2);

        // Rolling back an isolated scope keeps what it wrote: each change
        // stood on its own (§6.1).
        let req = request(SetMode::Isolated);
        let scope = now(state.open(req, no_checks));
        now(in_set_scope(req.scope, async {
            state.lock().await.write("c", "three")
        }));
        now(scope.rollback());
        assert_eq!(now(state.lock()).names.len(), 3);
    }

    #[test]
    fn deferring_only_inside_the_running_atomic_scope() {
        let state = seeded();
        assert!(!state.deferring());
        let req = request(SetMode::Atomic);
        let scope = now(state.open(req, no_checks));
        assert!(!state.deferring(), "not in the scope");
        assert!(now(in_set_scope(req.scope, async { state.deferring() })));
        let other = SetScopeId::next();
        assert!(!now(in_set_scope(other, async { state.deferring() })));
        drop(scope);
        assert!(!now(in_set_scope(req.scope, async { state.deferring() })));
    }
}
