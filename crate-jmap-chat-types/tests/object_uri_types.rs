//! `JMAP_URI_TASK_TYPE` agrees with the "JMAP Object URI Types" registry
//! (draft-atwood-jmap-chat-tasks-00), as collected in `jmap-object-uri`.

use jmap_chat_types::JMAP_URI_TASK_TYPE;
use jmap_object_uri::uri_type;

#[test]
fn task_type_matches_the_registry() {
    assert_eq!(
        uri_type(JMAP_URI_TASK_TYPE).map(|e| e.data_type),
        Some("Task")
    );
}
