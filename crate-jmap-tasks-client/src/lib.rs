// jmap-tasks-client — JMAP Tasks method implementations.
// Depends on jmap-base-client for transport, auth, and session.
// See PLAN.md for the full implementation plan.

/// Extension trait adding JMAP Tasks methods to [`jmap_base_client::JmapClient`].
///
/// Import this trait to use: `use jmap_tasks_client::JmapTasksExt;`
pub trait JmapTasksExt {
    // Methods will be added in implementation beads.
}

impl JmapTasksExt for jmap_base_client::JmapClient {}
