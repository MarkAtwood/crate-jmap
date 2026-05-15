//! Runtime detection of vendor-extras key collisions with typed fields.
//!
//! The workspace extras-preservation policy (see workspace AGENTS.md)
//! exposes a `#[serde(flatten)] extra: serde_json::Map<String, Value>`
//! field on every data-object type. A caller who programmatically
//! inserts a key into `extra` that matches one of the typed
//! wire-format field names produces a **duplicate JSON object key** on
//! serialize. RFC 8259 §4 leaves duplicate-key handling
//! implementation-defined; cross-implementation behavior varies
//! (serde_json last-wins, some validators reject, audit-log scrapers
//! may sample the first occurrence).
//!
//! The `validate_extras()` method on each affected type is a defensive
//! pre-serialize check that returns [`CollisionError`] if any `extra`
//! key shadows a typed field. Recommended usage:
//!
//! ```rust
//! # use jmap_contacts_types::{ContactCard, CollisionError};
//! # let card = ContactCard::default();
//! card.validate_extras()?;
//! let wire = serde_json::to_string(&card).unwrap();
//! # Ok::<(), CollisionError>(())
//! ```
//!
//! This helper exists because Rust cannot enforce "do not insert this
//! key" at compile time on a `pub` `serde_json::Map`. JMAP-glx8.25.

use std::fmt;

/// Error returned by `validate_extras()` when the `extra` map contains
/// one or more keys that shadow typed wire-format fields on the same
/// struct. The colliding keys are reported in deterministic
/// alphabetical order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollisionError {
    /// The colliding keys, sorted alphabetically for stable error
    /// messages and snapshot tests.
    keys: Vec<String>,
}

impl CollisionError {
    /// Construct a `CollisionError` from an iterator of colliding key
    /// names. The keys are deduplicated and sorted alphabetically.
    pub(crate) fn new<I, S>(keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut keys: Vec<String> = keys.into_iter().map(Into::into).collect();
        keys.sort();
        keys.dedup();
        Self { keys }
    }

    /// The colliding key names, in alphabetical order.
    #[must_use]
    pub fn keys(&self) -> &[String] {
        &self.keys
    }
}

impl fmt::Display for CollisionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "vendor-extras key(s) collide with typed wire-format field(s): {}",
            self.keys.join(", ")
        )
    }
}

impl std::error::Error for CollisionError {}

/// Internal helper: check a `serde_json::Map` against a slice of
/// reserved typed field names. Returns `Ok(())` when no key in the
/// map matches a reserved name; otherwise returns a `CollisionError`
/// listing every collision.
pub(crate) fn check(
    extra: &serde_json::Map<String, serde_json::Value>,
    reserved: &[&str],
) -> Result<(), CollisionError> {
    let collisions: Vec<&str> = reserved
        .iter()
        .copied()
        .filter(|name| extra.contains_key(*name))
        .collect();
    if collisions.is_empty() {
        Ok(())
    } else {
        Err(CollisionError::new(collisions))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collision_error_display_lists_keys_alphabetically() {
        let err = CollisionError::new(["uid", "id", "version"]);
        assert_eq!(err.keys(), &["id", "uid", "version"]);
        let msg = err.to_string();
        assert!(msg.contains("id, uid, version"), "got: {msg}");
    }

    #[test]
    fn collision_error_dedups_duplicate_input_keys() {
        let err = CollisionError::new(["id", "id", "uid"]);
        assert_eq!(err.keys(), &["id", "uid"]);
    }

    #[test]
    fn check_returns_ok_on_clean_extras() {
        let mut extra = serde_json::Map::new();
        extra.insert("acmeCorpFoo".into(), serde_json::json!("bar"));
        assert!(check(&extra, &["id", "uid"]).is_ok());
    }

    #[test]
    fn check_returns_ok_on_empty_extras() {
        let extra = serde_json::Map::new();
        assert!(check(&extra, &["id", "uid"]).is_ok());
    }

    #[test]
    fn check_returns_err_on_single_collision() {
        let mut extra = serde_json::Map::new();
        extra.insert("uid".into(), serde_json::json!("attacker"));
        let err = check(&extra, &["id", "uid"]).expect_err("collision must error");
        assert_eq!(err.keys(), &["uid"]);
    }

    #[test]
    fn check_returns_err_on_multiple_collisions() {
        let mut extra = serde_json::Map::new();
        extra.insert("uid".into(), serde_json::json!("a"));
        extra.insert("id".into(), serde_json::json!("b"));
        extra.insert("acmeCorpFoo".into(), serde_json::json!("c"));
        let err = check(&extra, &["id", "uid", "version"]).expect_err("collisions must error");
        // Both colliding reserved names returned, alphabetically.
        assert_eq!(err.keys(), &["id", "uid"]);
    }
}
