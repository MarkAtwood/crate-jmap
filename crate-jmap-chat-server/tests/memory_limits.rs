//! `MemoryBackend` bounds for long-running embedders (bd JMAP-it7g.3):
//! the change-log limit and the recorded-event limit.
//!
//! Oracle: RFC 8620 §5.2 cannotCalculateChanges: "The server cannot
//! calculate the changes from the state string given by the client.
//! Usually due to the client's state being too old". A state the kept
//! log still covers computes changes as before.

use jmap_chat_server::memory::MemoryBackend;
use jmap_chat_server::{ChatEvents, ChatStore};
use jmap_chat_types::CustomEmoji;
use jmap_server::{BackendChangesError, ChangesParams, JmapReads};
use jmap_types::{Id, State};

fn emoji(id: &Id, name: &str) -> CustomEmoji {
    let mut v = serde_json::json!({
        "name": name, "blobId": "b1", "createdBy": "u1",
        "createdAt": "2026-01-01T00:00:00Z"
    });
    v["id"] = serde_json::json!(id);
    serde_json::from_value(v).expect("fixture must decode")
}

async fn create_n(backend: &MemoryBackend, account: &Id, n: usize) -> Vec<State> {
    let mut states = Vec::new();
    for i in 0..n {
        states.push(
            backend
                .get_state::<CustomEmoji>(&(), account)
                .await
                .expect("state"),
        );
        backend
            .create_object::<CustomEmoji, _>(&(), account, &format!("c{i}"), |id: &Id| {
                emoji(id, &format!("party{i}"))
            })
            .await
            .expect("create");
    }
    states
}

#[tokio::test]
async fn change_log_limit_drops_the_oldest_states() {
    let account = Id::from("a1");
    let backend = MemoryBackend::new()
        .with_account(&account)
        .with_change_log_limit(2);
    let states = create_n(&backend, &account, 4).await;

    // The two newest changes are kept: from states[2] both are reported.
    let recent = backend
        .get_changes::<CustomEmoji>(&(), &account, ChangesParams::new(&states[2]))
        .await
        .expect("changes from a kept state");
    assert_eq!(recent.created.len(), 2, "{recent:?}");

    // Older states fall below the floor.
    for old in &states[..2] {
        let err = backend
            .get_changes::<CustomEmoji>(&(), &account, ChangesParams::new(old))
            .await
            .expect_err("dropped state");
        assert!(
            matches!(err, BackendChangesError::CannotCalculate),
            "{err:?}"
        );
    }
}

#[tokio::test]
async fn without_a_limit_every_state_is_kept() {
    let account = Id::from("a1");
    let backend = MemoryBackend::new().with_account(&account);
    let states = create_n(&backend, &account, 4).await;
    let all = backend
        .get_changes::<CustomEmoji>(&(), &account, ChangesParams::new(&states[0]))
        .await
        .expect("changes");
    assert_eq!(all.created.len(), 4, "{all:?}");
}

#[tokio::test]
async fn event_limit_keeps_the_newest_and_drains_are_public() {
    let backend = MemoryBackend::new().with_event_limit(0);
    let account = Id::from("a1");
    backend.register_account(&account);
    let event = jmap_chat_server::OutboundEvent::NewMessage {
        chat_id: Id::from("c"),
        message_id: Id::from("m"),
        sender_msg_id: Id::from("m"),
    };
    backend.on_outbound_event(&(), &account, &event).await;
    assert!(backend.drain_outbound_events().is_empty());

    let backend = MemoryBackend::new().with_event_limit(1);
    backend.on_outbound_event(&(), &account, &event).await;
    backend.on_outbound_event(&(), &account, &event).await;
    assert_eq!(backend.drain_outbound_events().len(), 1);
    assert!(backend.drain_outbound_events().is_empty(), "drained");
    assert!(backend.drain_lifecycle_events().is_empty());
}
