//! JMAP for Mail Sharing
//! ([draft-ietf-jmap-mail-sharing-02](https://datatracker.ietf.org/doc/html/draft-ietf-jmap-mail-sharing-02)):
//! facade crate re-exporting the RFC 9670 sharing crates it builds on,
//! under the extension's name.
//!
//! Mail sharing adds the RFC 9670 `shareWith` property and a `mayShare`
//! right to the RFC 8621 Mailbox. The Principals and `ShareNotification`s it
//! relies on are implemented by the `jmap-sharing-*` crates, which this
//! crate re-exports whole:
//!
//! - [`types`]: the [`jmap-sharing-types`](https://crates.io/crates/jmap-sharing-types),
//!   always enabled.
//! - `client`: [`jmap-sharing-client`](https://crates.io/crates/jmap-sharing-client),
//!   behind the `client` feature.
//! - `server`: [`jmap-sharing-server`](https://crates.io/crates/jmap-sharing-server),
//!   behind the `server` feature; the `memory` feature adds its in-memory
//!   reference backend.
//!
//! It also keeps [`CAPABILITY`], the one item the 0.0.1 placeholder
//! published. The Mailbox side of the draft lives in the mail crates:
//! `jmap-mail-types` types `Mailbox::share_with` and
//! `MailboxRights::may_share` and names the URI `JMAP_MAIL_SHARE_URI`;
//! `jmap-mail-server` enforces `mayShare` (its reference backend computes
//! `myRights` per caller from `shareWith`); `jmap-mail-client`'s
//! `SessionClient::with_mail_sharing` puts the URI in `using`.
//!
//! The component crates version in lockstep with this one.
//!
//! ```
//! use jmap_mail_sharing::types::{JMAP_PRINCIPALS_OWNER_URI, PrincipalsOwnerCapability};
//!
//! assert_eq!(jmap_mail_sharing::CAPABILITY, "urn:ietf:params:jmap:mail:share");
//! // §2: shareWith keys are Principal ids in the account the
//! // principals:owner capability names.
//! assert_eq!(JMAP_PRINCIPALS_OWNER_URI, "urn:ietf:params:jmap:principals:owner");
//! let owner: PrincipalsOwnerCapability = serde_json::from_str(
//!     r#"{"accountIdForPrincipal": "A1", "principalId": "P1"}"#,
//! )
//! .unwrap();
//! assert_eq!(serde_json::to_value(&owner.principal_id).unwrap(), "P1");
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

pub use jmap_sharing_types as types;

#[cfg(feature = "client")]
pub use jmap_sharing_client as client;

#[cfg(feature = "server")]
pub use jmap_sharing_server as server;

/// The mail sharing capability URI, `urn:ietf:params:jmap:mail:share`
/// (draft-ietf-jmap-mail-sharing-02 §1.2.1). Its Session and account
/// values are empty objects.
///
/// Kept from the 0.0.1 release under its name there.
pub const CAPABILITY: &str = "urn:ietf:params:jmap:mail:share";
