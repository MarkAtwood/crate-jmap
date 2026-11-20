//! JMAP Object History helpers for server backends
//! (draft-ietf-jmap-object-history-03 with the -04 editor's copy changes,
//! docs/design/object-history.md §2a).
//!
//! Pure and synchronous: no handlers, no backend trait, no async. The
//! handlers and the `ObjectHistoryBackend` trait live in `jmap-server`;
//! reference backends use these helpers to implement it.
//!
//! - [`select_versions`] and [`HistorySelector`]: which entries a history
//!   `Foo/get` returns for one id (§3).
//! - [`VersionLog`] and [`HistoryRetention`]: replaced and destroyed
//!   versions with retention, `expires`, the equal-millisecond collapse,
//!   the §8.2 rights cut and restore (§3.1, §5.1).
//! - [`HistoryStore`]: all of the above for a reference backend that
//!   stores objects as JSON, with the destroyed-query state and changes.
//! - [`fold_restore`]: where `Foo/changes` reports a restored id (-04
//!   §5.1).
//! - [`utc_date_from_millis`] and [`millis_from_utc_date`]: the log's
//!   millisecond clock as `UTCDate`s.
//!
//! # Example
//!
//! ```rust
//! use jmap_object_history_server::{HistorySelector, Selection, VersionLog};
//! use jmap_types::Id;
//! use serde_json::json;
//!
//! let (acc, id) = (Id::from("a1"), Id::from("c1"));
//! let mut log = VersionLog::default();
//! log.record_replace("ContactCard", &acc, &id, json!({"name": "Robert"}), 1_000);
//! let sel = HistorySelector::new().include_replaced(true);
//! let Selection::Entries(entries) =
//!     log.select("ContactCard", &acc, &id, Some(json!({"name": "Bob"})), &sel, None).unwrap()
//! else {
//!     panic!("found")
//! };
//! assert_eq!(entries.len(), 2);
//! assert_eq!(entries[0].1.replaced.as_ref().unwrap().as_ref(), "1970-01-01T00:00:01Z");
//! assert_eq!(entries[1].1.replaced, None);
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

mod fold;
mod log;
mod select;
mod store;
mod time;

pub use fold::{ChangeEvent, ChangeKind, fold_restore};
pub use log::{DestroyedObject, HistoryRetention, VersionLog};
pub use select::{HistorySelector, Selection, Version, select_versions};
pub use store::{HistoryStore, SelectedHistory};
pub use time::{millis_from_utc_date, utc_date_from_millis};
