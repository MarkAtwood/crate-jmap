//! JMAP conditional and atomic Foo/set
//! ([draft-ietf-jmap-conditional-02](https://datatracker.ietf.org/doc/html/draft-ietf-jmap-conditional-02)):
//! facade crate re-exporting the conditional support of
//! [`jmap-types`](https://crates.io/crates/jmap-types) and
//! [`jmap-server`](https://crates.io/crates/jmap-server) under the
//! extension's name.
//!
//! Conditional support has no crate family of its own: it is the
//! `conditional` module of `jmap-types` plus a set of `jmap-server` items.
//! This crate re-exports exactly those:
//!
//! - [`types`]: `jmap_types::conditional`, always enabled.
//! - `server`: the backend hooks and registration items of `jmap-server`,
//!   behind the `server` feature; the `scoped-state` feature adds
//!   `ScopedState`, `ScopeGate` and `ScopedGuard`.
//!
//! There is no `client` feature: a client sends the two Foo/set arguments
//! as a [`types::ConditionalSetArgs`] and reads the new errors with the
//! `JmapError` methods that `jmap-types` defines.
//!
//! It also keeps [`CAPABILITY`], the one item the 0.0.1 placeholder
//! published. The re-exported crates version in lockstep with this one.
//!
//! ```
//! use jmap_conditional::types::{ConditionalSetArgs, JMAP_CONDITIONAL_URI};
//!
//! assert_eq!(JMAP_CONDITIONAL_URI, "urn:ietf:params:jmap:conditional");
//! assert_eq!(jmap_conditional::CAPABILITY, JMAP_CONDITIONAL_URI);
//! let args: ConditionalSetArgs = serde_json::from_str(
//!     r#"{"ifUnchangedBy": {"M1": {"name": "Inbox"}}, "atomic": true}"#,
//! )
//! .unwrap();
//! assert!(args.atomic);
//! assert!(args.if_unchanged_by.contains_key("M1"));
//! ```

#![forbid(unsafe_code)]

pub use jmap_types::conditional as types;

#[cfg(feature = "server")]
pub mod server {
    //! The draft-ietf-jmap-conditional-02 items of `jmap-server`: the
    //! [`ConditionalBackend`] hooks, the [`SetScope`] a backend opens, and the
    //! opt-in [`SetSupport`] registration.
    //!
    //! The dispatcher methods that go with them (`attach_set_support`,
    //! `conditional_ready`, `conditional_gaps`) are inherent methods of
    //! `jmap_server::Dispatcher`. Each extension server crate's
    //! `register_<x>_conditional` stays in that crate;
    //! [`register_push_subscription_conditional`] is the one `jmap-server`
    //! defines itself.

    pub use jmap_server::{
        BeginSetError, CommitError, CommitFuture, ConditionalBackend, DeferredFailure,
        RegistrationError, RollbackFuture, SetMode, SetOp, SetScope, SetScopeId, SetScopeRequest,
        SetSupport, current_set_scope, register_push_subscription_conditional,
        serialized_writes_scope,
    };

    #[cfg(feature = "scoped-state")]
    pub use jmap_server::{ScopeGate, ScopedGuard, ScopedState};
}

/// The conditional capability URI, `urn:ietf:params:jmap:conditional`.
///
/// Kept from the 0.0.1 release under its name there; new code names
/// [`types::JMAP_CONDITIONAL_URI`].
pub const CAPABILITY: &str = types::JMAP_CONDITIONAL_URI;
