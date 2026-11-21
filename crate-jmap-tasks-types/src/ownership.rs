//! Who owns a Task, for the `TaskList` `mayWriteOwn` right.
//!
//! draft-ietf-jmap-tasks-06 §3 defines a task owner through a
//! "participant" property and the "chair" role. A Task has no
//! "participant" property, and "chair" is not the role Calendars uses, so
//! this module follows the JMAP Calendars definition instead
//! (draft-ietf-jmap-calendars-34 §4, jmapio/jmap #461 / PR #462):
//!
//! > The user is an *owner* for an event if the CalendarEvent object has a
//! > "participants" property, and one of the Participant objects both:
//! > a. Has the "owner" role.
//! > b. Corresponds to one of the user's ParticipantIdentity objects in
//! > the account (as per Section 3).
//! >
//! > An event has no owner if its "participants" property is null or
//! > omitted, or if none of the Participant objects have the "owner" role.
//!
//! A participant corresponds to an identity by `calendarAddress`
//! (draft-ietf-jmap-calendars-34 §3; jmapio/jmap #470).

use crate::Task;

/// The participants of `task` that have the `"owner"` role.
///
/// `participants` is untyped JSON (the Sloppy-Value pattern), so an entry
/// that is not an object, or whose `roles` does not map `"owner"` to
/// `true`, is not an owner.
fn owner_participants(task: &Task) -> impl Iterator<Item = &serde_json::Value> {
    task.participants
        .as_ref()
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flat_map(serde_json::Map::values)
        .filter(|p| {
            p.get("roles")
                .and_then(|roles| roles.get("owner"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        })
}

/// `true` if `task` has an owner: its `participants` holds at least one
/// participant with the `"owner"` role.
///
/// A task whose `participants` is null or omitted, or none of whose
/// participants has the `"owner"` role, has no owner
/// (draft-ietf-jmap-calendars-34 §4, as jmapio/jmap PR #462 proposes for
/// Tasks). Under `mayWriteOwn` anyone with the right may write a task that
/// has no owner (draft-ietf-jmap-tasks-06 §3).
#[must_use]
pub fn task_has_owner(task: &Task) -> bool {
    owner_participants(task).next().is_some()
}

/// `true` if the user whose `ParticipantIdentity` `calendarAddress` values
/// are `user_calendar_addresses` is an owner of `task`: some participant
/// has the `"owner"` role and a `calendarAddress` equal to one of them.
///
/// See the module docs for the definition (draft-ietf-jmap-calendars-34
/// §4, jmapio/jmap #461). The backend passes the calendar addresses of
/// the caller's `ParticipantIdentity` objects in the account.
///
/// Addresses compare as exact strings, as the Calendars availability
/// helpers do. Calendars -34 §3 matches them "after syntax-based
/// normalization, as per Section 6.2.2 of \[RFC3986\]"; a backend that
/// stores identities in another form normalizes them, and a participant
/// address in another form does not match.
#[must_use]
pub fn is_task_owner(task: &Task, user_calendar_addresses: &[&str]) -> bool {
    owner_participants(task).any(|p| {
        p.get("calendarAddress")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|a| user_calendar_addresses.contains(&a))
    })
}

#[cfg(test)]
mod tests {
    use super::{is_task_owner, task_has_owner};
    use crate::Task;
    use serde_json::json;

    const ME: &str = "mailto:me@example.com";

    fn task(participants: serde_json::Value) -> Task {
        serde_json::from_value(json!({ "@type": "Task", "participants": participants }))
            .expect("hand-written Task deserializes")
    }

    /// Oracle: PR #462 "A task has no owner if its "participants" property
    /// is null or omitted".
    #[test]
    fn omitted_participants_means_no_owner() {
        let t: Task = serde_json::from_value(json!({ "@type": "Task" })).expect("Task");
        assert!(!task_has_owner(&t));
        assert!(!is_task_owner(&t, &[ME]));
    }

    /// Oracle: PR #462 "... null or omitted".
    #[test]
    fn null_participants_means_no_owner() {
        let t = task(serde_json::Value::Null);
        assert!(!task_has_owner(&t));
    }

    /// Oracle: PR #462 "... or if none of the Participant objects have the
    /// "owner" role." The -06 "chair" role does not make an owner.
    #[test]
    fn participants_without_owner_role_means_no_owner() {
        let t = task(json!({
            "p1": { "@type": "Participant", "calendarAddress": ME,
                    "roles": { "chair": true, "attendee": true } }
        }));
        assert!(!task_has_owner(&t));
        assert!(!is_task_owner(&t, &[ME]));
    }

    /// Oracle: PR #462 "one of the Participant objects both: 1. Has the
    /// "owner" role. 2. Corresponds to one of the user's
    /// ParticipantIdentity objects", matched by calendarAddress (#470).
    #[test]
    fn owner_role_with_users_address_is_owner() {
        let t = task(json!({
            "p1": { "@type": "Participant", "calendarAddress": ME,
                    "roles": { "owner": true } }
        }));
        assert!(task_has_owner(&t));
        assert!(is_task_owner(&t, &[ME]));
    }

    /// Oracle: PR #462 condition 2: an owner that is someone else makes the
    /// task owned, but not by this user.
    #[test]
    fn owner_role_with_other_address_is_not_users() {
        let t = task(json!({
            "p1": { "@type": "Participant", "calendarAddress": "mailto:boss@example.com",
                    "roles": { "owner": true } },
            "p2": { "@type": "Participant", "calendarAddress": ME,
                    "roles": { "attendee": true } }
        }));
        assert!(task_has_owner(&t));
        assert!(!is_task_owner(&t, &[ME]));
    }
}
