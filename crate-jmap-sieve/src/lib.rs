//! JMAP for Sieve Scripts
//! ([RFC 9661](https://www.rfc-editor.org/rfc/rfc9661)): facade crate
//! re-exporting the `sieve` feature of the mail crates under the
//! extension's name.
//!
//! Sieve has no crate family of its own: it is the `sieve` feature of
//! [`jmap-mail-types`](https://crates.io/crates/jmap-mail-types) and
//! [`jmap-mail-server`](https://crates.io/crates/jmap-mail-server). This
//! crate enables that feature and re-exports exactly its items:
//!
//! - [`types`]: the Sieve wire types of `jmap-mail-types`, always enabled.
//! - `server`: the `SieveBackend` trait, the `SieveScript/*` handlers and their registration functions
//!   from `jmap-mail-server`, behind the `server` feature.
//!
//! There is no `client` feature: `jmap-mail-client` has no Sieve bindings.
//!
//! It also keeps [`CAPABILITY`], the one item the 0.0.1 placeholder
//! published. The re-exported crates version in lockstep with this one.
//!
//! ```
//! use jmap_sieve::types::{JMAP_SIEVE_SCRIPTS_URI, SieveCapability};
//!
//! assert_eq!(JMAP_SIEVE_SCRIPTS_URI, "urn:ietf:params:jmap:sieve");
//! assert_eq!(jmap_sieve::CAPABILITY, JMAP_SIEVE_SCRIPTS_URI);
//! let cap: SieveCapability =
//!     serde_json::from_str(r#"{"implementation": "ACME Email Filtering"}"#).unwrap();
//! assert_eq!(cap.implementation, "ACME Email Filtering");
//! ```

#![forbid(unsafe_code)]

pub mod types {
    //! The RFC 9661 wire types of `jmap-mail-types`: the [`SieveScript`]
    //! object and its property names, and the Session and account
    //! capabilities.

    pub use jmap_mail_types::{
        JMAP_SIEVE_SCRIPTS_URI, SieveAccountCapability, SieveCapability, SieveScript,
        SieveScriptProperty,
    };
}

#[cfg(feature = "server")]
pub mod server {
    //! The RFC 9661 items of `jmap-mail-server`: the [`SieveBackend`] trait,
    //! the `SieveScript/get`, `/set`, `/query` and `/validate` handlers (plain
    //! and with object history), and [`register_sieve_handlers`] and
    //! [`register_sieve_handlers_with_history`].
    //!
    //! Conditional `SieveScript/set` is attached by
    //! `jmap_mail_server::register_mail_conditional`, which covers every mail
    //! `/set` method and is not re-exported here. The memory reference backend
    //! implements [`SieveBackend`] when `jmap-mail-server`'s `memory` and
    //! `sieve` features are both on; it is in `jmap_mail_server::memory`.

    pub use jmap_mail_server::{
        SieveBackend, handle_sieve_get, handle_sieve_get_with_history, handle_sieve_query,
        handle_sieve_query_with_history, handle_sieve_set, handle_sieve_set_with_history,
        handle_sieve_validate, register_sieve_handlers, register_sieve_handlers_with_history,
    };
}

/// The Sieve capability URI, `urn:ietf:params:jmap:sieve`.
///
/// Kept from the 0.0.1 release under its name there; new code names
/// [`types::JMAP_SIEVE_SCRIPTS_URI`].
pub const CAPABILITY: &str = types::JMAP_SIEVE_SCRIPTS_URI;
