//! JMAP Mail
//! ([RFC 8621](https://www.rfc-editor.org/rfc/rfc8621)):
//! umbrella crate re-exporting the component crates, so one dependency
//! covers them.
//!
//! - [`types`]: the [`jmap-mail-types`](https://crates.io/crates/jmap-mail-types),
//!   always enabled.
//! - `client`: [`jmap-mail-client`](https://crates.io/crates/jmap-mail-client),
//!   behind the `client` feature.
//! - `server`: [`jmap-mail-server`](https://crates.io/crates/jmap-mail-server),
//!   behind the `server` feature; the `memory` feature adds its in-memory
//!   reference backend.
//!
//! `emailpush`, `mdn` and `sieve` turn on the same features of the
//! component crates that are enabled.
//!
//! The component crates version in lockstep with this one.
//!
//! ```
//! use jmap_mail::types::JMAP_MAIL_URI;
//!
//! assert_eq!(JMAP_MAIL_URI, "urn:ietf:params:jmap:mail");
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

pub use jmap_mail_types as types;

#[cfg(feature = "client")]
pub use jmap_mail_client as client;

#[cfg(feature = "server")]
pub use jmap_mail_server as server;
