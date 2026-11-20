//! The caller context the in-repo reference `MemoryBackend`s accept.

use jmap_types::Id;

/// A caller context the in-repo reference `MemoryBackend`s accept as their
/// [`JmapAccounts::CallerCtx`](crate::JmapAccounts::CallerCtx): anything
/// that names the caller's Principal, or names none.
///
/// Every reference backend in this workspace is generic over one
/// `C: MemoryCaller`, with [`Id`] as the default, and its
/// [`JmapAccounts::principal_id`](crate::JmapAccounts::principal_id)
/// returns [`MemoryCaller::principal`]. A deployment therefore picks one
/// caller type and every family's memory backend takes it, so one
/// [`Dispatcher`](crate::Dispatcher) of that type hosts all their
/// handlers.
///
/// - [`Id`]: the caller is the Principal with that id. A backend that
///   models per-user rights (sharing, owners, moderators) checks them
///   against it; a single-user backend treats every caller as the account
///   owner.
/// - `()`: a deployment without identity. Every caller acts as the account
///   owner, with all rights.
///
/// A production deployment implements its own backends against its own
/// caller type and does not need this trait; it exists so the reference
/// backends agree on one.
pub trait MemoryCaller: Clone + Send + Sync + 'static {
    /// The caller's Principal id, or `None` for a deployment without
    /// identity, whose every caller acts as the account owner.
    fn principal(&self) -> Option<&Id>;
}

/// The caller is the Principal with this id.
impl MemoryCaller for Id {
    fn principal(&self) -> Option<&Id> {
        Some(self)
    }
}

/// A deployment without identity: every caller acts as the account owner.
impl MemoryCaller for () {
    fn principal(&self) -> Option<&Id> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_names_itself_and_unit_names_nobody() {
        let id = Id::from("alice");
        assert_eq!(id.principal(), Some(&Id::from("alice")));
        assert_eq!(().principal(), None);
    }
}
