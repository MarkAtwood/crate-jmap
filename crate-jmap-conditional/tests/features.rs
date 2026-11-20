//! The facade's re-exports under each feature set.
//!
//! Oracles: draft-ietf-jmap-conditional-02 (§2.1 the capability URI, §3.1
//! the `ifUnchangedBy` and `atomic` arguments, §4.2 and §4.3 the error
//! codes) and the crate's documented feature table.

use jmap_conditional::types::{
    ATOMIC_FAILURE, CANNOT_APPLY_ATOMICALLY, ConditionalSetArgs, JMAP_CONDITIONAL_URI,
};

#[test]
fn types_are_always_there() {
    assert_eq!(JMAP_CONDITIONAL_URI, "urn:ietf:params:jmap:conditional");
    assert_eq!(jmap_conditional::CAPABILITY, JMAP_CONDITIONAL_URI);
    assert_eq!(ATOMIC_FAILURE, "atomicFailure");
    assert_eq!(CANNOT_APPLY_ATOMICALLY, "cannotApplyAtomically");
    // §3.1: a creation-id key with the null ("does not exist") precondition.
    let args: ConditionalSetArgs =
        serde_json::from_str(r##"{"ifUnchangedBy": {"#c1": null}, "atomic": true}"##).unwrap();
    assert!(args.atomic);
    assert_eq!(args.if_unchanged_by.get("#c1"), Some(&None));
    assert!(ConditionalSetArgs::default().is_default());
}

#[cfg(feature = "server")]
#[test]
fn server_is_reexported() {
    use jmap_conditional::server::{
        BeginSetError, SetMode, SetScopeRequest, serialized_writes_scope,
    };

    // §4.3: a scope that cannot undo writes refuses an atomic call and
    // accepts an isolated one.
    let args = serde_json::json!({});
    let atomic = SetScopeRequest::new("Mailbox/set", "Mailbox", None, SetMode::Atomic, &args);
    assert!(matches!(
        serialized_writes_scope::<()>(atomic),
        Err(BeginSetError::CannotApplyAtomically)
    ));
    let isolated = SetScopeRequest::new("Mailbox/set", "Mailbox", None, SetMode::Isolated, &args);
    assert!(serialized_writes_scope::<()>(isolated).is_ok());
}

#[cfg(feature = "scoped-state")]
#[test]
fn scoped_state_is_reexported() {
    use jmap_conditional::server::{ScopeGate, ScopedState};

    let gate = ScopeGate::new();
    let state = ScopedState::with_gate(7u32, gate.clone());
    assert!(std::sync::Arc::ptr_eq(state.gate(), &gate));
    assert_eq!(*state.lock_unscoped(), 7);
}
