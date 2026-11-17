//! How a `Foo/changes` window reports an object that was restored.

/// One change to an object in a `Foo/changes` window, oldest first.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeEvent {
    /// Created by a create, copy or import.
    Created,
    /// Updated.
    Updated,
    /// Destroyed, by any route.
    Destroyed,
    /// Restored from history (§5.1).
    Restored,
}

/// Where `Foo/changes` reports an id.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// In `created`.
    Created,
    /// In `updated`.
    Updated,
    /// In `destroyed`.
    Destroyed,
}

/// Classify an id from its changes since `sinceState`, oldest first, the
/// -04 §5.1 way: "exactly as if the object had never been destroyed
/// between them".
///
/// The object existed at `sinceState` unless its first change created or
/// restored it, and exists now unless its last change destroyed it. In
/// both ⇒ `updated`; now only ⇒ `created`; at `sinceState` only ⇒
/// `destroyed`; in neither ⇒ `None`: the server "SHOULD NOT report the id
/// at all" (the MAY of reporting it as created and destroyed is not
/// taken). `None` too for no changes.
pub fn fold_restore(events: &[ChangeEvent]) -> Option<ChangeKind> {
    let first = events.first()?;
    let last = events.last()?;
    let existed = !matches!(first, ChangeEvent::Created | ChangeEvent::Restored);
    let exists = !matches!(last, ChangeEvent::Destroyed);
    match (existed, exists) {
        (true, true) => Some(ChangeKind::Updated),
        (false, true) => Some(ChangeKind::Created),
        (true, false) => Some(ChangeKind::Destroyed),
        (false, false) => None,
    }
}
