//! JMAP Email Delivery Push
//! ([draft-ietf-jmap-emailpush-05](https://datatracker.ietf.org/doc/html/draft-ietf-jmap-emailpush-05)):
//! facade crate re-exporting the `emailpush` feature of the mail crates
//! under the extension's name.
//!
//! EmailPush has no crate family of its own: it is the `emailpush` feature
//! of [`jmap-mail-types`](https://crates.io/crates/jmap-mail-types),
//! [`jmap-mail-client`](https://crates.io/crates/jmap-mail-client) and
//! [`jmap-mail-server`](https://crates.io/crates/jmap-mail-server)
//! (docs/design/DECISIONS.md decision 3). This crate enables that feature
//! and re-exports exactly its items:
//!
//! - [`types`]: the wire types of `jmap-mail-types`, always enabled.
//! - `client`: the push subscription input and payload decoding of
//!   `jmap-mail-client`, behind the `client` feature.
//! - `server`: `jmap_mail_server::emailpush`, behind the `server` feature.
//!
//! It also keeps [`CAPABILITY`], the one item the 0.0.1 placeholder
//! published. The re-exported crates version in lockstep with this one.
//!
//! ```
//! use jmap_emailpush::types::{EmailPushConfig, JMAP_EMAILPUSH_URI};
//!
//! assert_eq!(JMAP_EMAILPUSH_URI, "urn:ietf:params:jmap:emailpush");
//! assert_eq!(jmap_emailpush::CAPABILITY, JMAP_EMAILPUSH_URI);
//! let config: EmailPushConfig = serde_json::from_str(
//!     r#"{"properties": ["id", "subject"], "urgency": "high"}"#,
//! )
//! .unwrap();
//! assert!(config.filter.is_none());
//! assert_eq!(config.properties, ["id", "subject"]);
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

pub mod types {
    //! The EmailPush wire types of `jmap-mail-types`: the capability, the
    //! pushed [`EmailPush`] object, the PushSubscription `emailPush` map's
    //! [`EmailPushConfig`] and its accessors.

    pub use jmap_mail_types::{
        EMAIL_PUSH_PROPERTY, EmailPush, EmailPushAccountCapability, EmailPushCapability,
        EmailPushConfig, JMAP_EMAILPUSH_URI, email_push, set_email_push,
    };
}

#[cfg(feature = "client")]
pub mod client {
    //! The EmailPush items of `jmap-mail-client`.
    //!
    //! The two calls that go with them,
    //! `SessionClient::email_push_subscription_create` and
    //! `SessionClient::set_email_push`, are inherent methods of
    //! `jmap_mail_client::SessionClient`.

    pub use jmap_mail_client::{EmailPushSubscriptionInput, PushPayload, parse_push_payload};
}

#[cfg(feature = "server")]
pub use jmap_mail_server::emailpush as server;

/// The EmailPush capability URI, `urn:ietf:params:jmap:emailpush`.
///
/// Kept from the 0.0.1 release under its name there; new code names
/// [`types::JMAP_EMAILPUSH_URI`].
pub const CAPABILITY: &str = types::JMAP_EMAILPUSH_URI;
