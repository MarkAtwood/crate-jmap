//! RFC 8621 §3 Thread object.
//!
//! Provides [`Thread`] — groups related [`crate::Email`] objects by
//! conversation thread.

use jmap_types::Id;
use serde::{Deserialize, Serialize};

/// A Thread object as defined in RFC 8621 §3.
///
/// Groups related Email objects by conversation thread. The `emailIds` field
/// lists member Email ids sorted oldest-first by `receivedAt`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    /// The id of the Thread (immutable; server-set).
    pub id: Id,
    /// The ids of the Emails in the Thread, sorted oldest-first by `receivedAt` (server-set).
    pub email_ids: Vec<Id>,
}

impl Thread {
    /// Construct a [`Thread`] from its two required fields.
    pub fn new(id: Id, email_ids: Vec<Id>) -> Self {
        Self { id, email_ids }
    }
}
