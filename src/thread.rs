use jmap_types::Id;
use serde::{Deserialize, Serialize};

/// A Thread object as defined in RFC 8621 §3.
///
/// Groups related Email objects by conversation thread. The `emailIds` field
/// lists member Email ids sorted oldest-first by `receivedAt`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Thread {
    /// The id of the Thread (immutable; server-set).
    pub id: Id,
    /// The ids of the Emails in the Thread, sorted oldest-first by `receivedAt` (server-set).
    pub email_ids: Vec<Id>,
}
