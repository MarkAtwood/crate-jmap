//! JMAP Message Disposition Notifications
//! ([RFC 9007](https://www.rfc-editor.org/rfc/rfc9007)): facade crate
//! re-exporting the `mdn` feature of the mail crates under the
//! extension's name.
//!
//! MDN has no crate family of its own: it is the `mdn` feature of
//! [`jmap-mail-types`](https://crates.io/crates/jmap-mail-types) and
//! [`jmap-mail-server`](https://crates.io/crates/jmap-mail-server). This
//! crate enables that feature and re-exports exactly its items:
//!
//! - [`types`]: the MDN wire types of `jmap-mail-types`, always enabled.
//! - `server`: `jmap_mail_server::mdn` and `register_mdn_handlers`, behind
//!   the `server` feature.
//!
//! There is no `client` feature: `jmap-mail-client` has no MDN bindings.
//!
//! It also keeps [`CAPABILITY`], the one item the 0.0.1 placeholder
//! published. The re-exported crates version in lockstep with this one.
//!
//! ```
//! use jmap_mdn::types::{Disposition, DispositionType, JMAP_MDN_URI};
//!
//! assert_eq!(JMAP_MDN_URI, "urn:ietf:params:jmap:mdn");
//! assert_eq!(jmap_mdn::CAPABILITY, JMAP_MDN_URI);
//! let d: Disposition = serde_json::from_str(
//!     r#"{"actionMode": "manual-action", "sendingMode": "mdn-sent-manually",
//!         "type": "displayed"}"#,
//! )
//! .unwrap();
//! assert_eq!(d.type_, DispositionType::Displayed);
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

pub mod types {
    //! The RFC 9007 wire types of `jmap-mail-types`: the [`Mdn`] object, its
    //! [`Disposition`], and the `MDN/send` and `MDN/parse` requests and
    //! responses.

    pub use jmap_mail_types::{
        ActionMode, Disposition, DispositionType, JMAP_MDN_URI, Mdn, MdnParseRequest,
        MdnParseResponse, MdnSendRequest, MdnSendResponse, SendingMode,
    };
}

#[cfg(feature = "server")]
pub mod server {
    //! The RFC 9007 items of `jmap-mail-server`: the [`MdnBackend`] trait, the
    //! `MDN/send` and `MDN/parse` handlers, and [`register_mdn_handlers`].
    //!
    //! The memory reference backend implements [`MdnBackend`] when
    //! `jmap-mail-server`'s `memory` and `mdn` features are both on; it is in
    //! `jmap_mail_server::memory` and is not re-exported here.

    pub use jmap_mail_server::mdn::{
        MDN_PARSE_MAX_BLOB_IDS, MdnBackend, MdnParseResult, MdnSendResult, handle_mdn_parse,
        handle_mdn_send,
    };
    pub use jmap_mail_server::register_mdn_handlers;
}

/// The MDN capability URI, `urn:ietf:params:jmap:mdn`.
///
/// Kept from the 0.0.1 release under its name there; new code names
/// [`types::JMAP_MDN_URI`].
pub const CAPABILITY: &str = types::JMAP_MDN_URI;
