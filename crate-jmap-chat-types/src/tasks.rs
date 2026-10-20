//! Chat-side shapes for the JMAP Chat Tasks binding
//! (draft-atwood-jmap-chat-tasks-00).
//!
//! - [`Space::task_list_id`] reads the optional `taskListId` Space
//!   extension (§taskListId). The value lives in [`Space::extra`], where
//!   the server's Space extension hook stores it.
//! - [`task_chat_id`] reads the optional `chatId` Task extension
//!   (§chatId) from a JMAP Tasks Task's extension map. This crate does not
//!   depend on `jmap-tasks-types`, so the accessor takes the map (the
//!   `extra` field of `jmap_tasks_types::Task`) rather than the Task.
//! - [`TaskMetadata`] is the typed `metadata` shape of a
//!   [`MessageAction`](crate::MessageAction) whose `type` is
//!   [`CHAT_CAP_TASK`](crate::vocabulary::CHAT_CAP_TASK). Cast with
//!   `serde_json::from_value` at the consumer boundary. The draft treats
//!   both `uri` and `metadata` as "peer-supplied and untrusted".
//! - [`JmapUri::task_ids`] extracts the `taskListId` and `taskId` from a
//!   `jmap:task:<accountId>:<taskListId>:<taskId>` URI.
//!
//! The capability object is [`ChatTasksCapability`](crate::ChatTasksCapability).

use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

use crate::{JmapUri, Space};

/// Wire name of the Space extension property binding a TaskList
/// (draft-atwood-jmap-chat-tasks-00 §taskListId).
pub const SPACE_TASK_LIST_ID_PROPERTY: &str = "taskListId";

/// Wire name of the Task extension property back-referencing a discussion
/// Chat (draft-atwood-jmap-chat-tasks-00 §chatId).
pub const TASK_CHAT_ID_PROPERTY: &str = "chatId";

/// The `<type>` segment of a `jmap:` URI that references a Task
/// (draft-atwood-jmap-chat-tasks-00 §MessageAction format).
pub const JMAP_URI_TASK_TYPE: &str = "task";

impl Space {
    /// The `taskListId` of the TaskList bound to this Space
    /// (draft-atwood-jmap-chat-tasks-00 §taskListId), read from
    /// [`extra`](Self::extra).
    ///
    /// Returns `Ok(None)` when the property is absent or `null` ("When
    /// absent, this Space has no bound TaskList").
    ///
    /// # Errors
    ///
    /// Returns the deserialization error when the value is not a String.
    pub fn task_list_id(&self) -> Result<Option<Id>, serde_json::Error> {
        self.extra
            .get(SPACE_TASK_LIST_ID_PROPERTY)
            .map_or(Ok(None), Option::<Id>::deserialize)
    }
}

/// The `chatId` of the discussion Chat a Task back-references
/// (draft-atwood-jmap-chat-tasks-00 §chatId), read from the Task's
/// extension map (`jmap_tasks_types::Task::extra`).
///
/// Returns `Ok(None)` when the property is absent or `null` ("When absent,
/// this Task has no associated discussion thread").
///
/// # Errors
///
/// Returns the deserialization error when the value is not a String.
pub fn task_chat_id(
    task_extra: &serde_json::Map<String, serde_json::Value>,
) -> Result<Option<Id>, serde_json::Error> {
    task_extra
        .get(TASK_CHAT_ID_PROPERTY)
        .map_or(Ok(None), Option::<Id>::deserialize)
}

impl JmapUri {
    /// The `(taskListId, taskId)` pair of a
    /// `jmap:task:<accountId>:<taskListId>:<taskId>` URI
    /// (draft-atwood-jmap-chat-tasks-00 §MessageAction format).
    ///
    /// Returns `None` unless the type is `task` and exactly two segments
    /// follow the account.
    pub fn task_ids(&self) -> Option<(&Id, &Id)> {
        match self.object_ids() {
            [task_list_id, task_id] if self.data_type() == JMAP_URI_TASK_TYPE => {
                Some((task_list_id, task_id))
            }
            _ => None,
        }
    }
}

/// `metadata` of a `urn:jmap:chat:cap:task` MessageAction
/// (draft-atwood-jmap-chat-tasks-00 §MessageAction format).
///
/// Every key is optional: the metadata "MAY include" them. Other keys land
/// in [`extra`](Self::extra): "Servers MAY include other type-specific
/// keys; clients MUST ignore unknown keys."
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskMetadata {
    /// `status` (String): the Task's current status, e.g. `"needs-action"`,
    /// `"in-process"`, `"completed"` or `"cancelled"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// `due` (UTCDate): the Task's due date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<UTCDate>,
    /// `assigneeIds` (String[]): each entry is a ChatContact id, or the
    /// accountId for the account owner. Between servers the entries carry
    /// identities instead (§Federation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee_ids: Option<Vec<Id>>,
    /// `priority` (Integer): priority value per JMAP Tasks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<i64>,
    /// Catch-all for other type-specific metadata keys. Preserves unknown
    /// fields across deserialize/serialize round-trip per workspace
    /// extras-preservation policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
