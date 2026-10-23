//! `Space/join` by invite code redeems the invite atomically through
//! `ChatBackend::join_space_with_invite` (bd:JMAP-x2gd.2).
//!
//! Oracle: draft-atwood-jmap-chat-00 §Space/join: "The `uses` increment
//! and membership insertion MUST be performed within a single atomic
//! operation so that concurrent redemptions cannot exceed `maxUses`."
//!
//! `MemoryBackend` stores invites per account and the joining member is
//! always the account's owner, so two distinct joiners racing for the
//! last use cannot be modelled here. These tests pin the properties that
//! can be: both effects land together, the backend re-checks `maxUses`
//! itself, a refusal writes nothing, and concurrent redemptions neither
//! lose nor double the increment.

use jmap_chat_server::memory::MemoryBackend;
use jmap_chat_server::{ChatBackend, InviteJoinRefusal, JmapBackend, handle_space_join};
use jmap_chat_types::{Space, SpaceInvite};
use jmap_types::{Id, UTCDate};
use serde_json::json;

const ACCOUNT_ID: &str = "a1";
const SPACE_ID: &str = "s1";
const INVITE_ID: &str = "inv1";
const CODE: &str = "0123456789abcdef0123456789abcdef";

/// A backend with an empty Space and one invite for it at `uses`.
fn seeded(uses: u64, max_uses: u64) -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend.insert_object_for_test(
        "Space",
        ACCOUNT_ID,
        SPACE_ID,
        json!({
            "id": SPACE_ID,
            "name": "Test Space",
            "createdAt": "2026-01-01T00:00:00Z",
            "memberCount": 0,
            "categories": [],
            "uncategorizedChannelIds": [],
            "isPublic": false,
            "isPubliclyPreviewable": false,
            "roles": [],
            "members": [],
        }),
    );
    backend.insert_object_for_test(
        "SpaceInvite",
        ACCOUNT_ID,
        INVITE_ID,
        json!({
            "id": INVITE_ID,
            "code": CODE,
            "spaceId": SPACE_ID,
            "createdBy": ACCOUNT_ID,
            "uses": uses,
            "maxUses": max_uses,
            "createdAt": "2026-01-01T00:00:00Z",
        }),
    );
    backend
}

/// `(member ids, invite uses, Space state, SpaceInvite state)`.
async fn snapshot(backend: &MemoryBackend) -> (Vec<String>, u64, String, String) {
    let acct = Id::from(ACCOUNT_ID);
    let (spaces, _) = backend
        .get_objects::<Space>(&(), &acct, Some(&[Id::from(SPACE_ID)]), None)
        .await
        .unwrap();
    let (invites, _) = backend
        .get_objects::<SpaceInvite>(&(), &acct, Some(&[Id::from(INVITE_ID)]), None)
        .await
        .unwrap();
    let members = spaces[0]
        .members
        .iter()
        .map(|m| m.id.as_ref().to_owned())
        .collect();
    let space_state = backend.get_state::<Space>(&(), &acct).await.unwrap();
    let invite_state = backend.get_state::<SpaceInvite>(&(), &acct).await.unwrap();
    (
        members,
        invites[0].uses,
        space_state.as_ref().to_owned(),
        invite_state.as_ref().to_owned(),
    )
}

/// "... atomically increment `uses` and add the caller to the Space's
/// member list": one join produces both effects, each recorded as a
/// change.
#[tokio::test]
async fn join_adds_member_and_counts_use_together() {
    let backend = seeded(0, 2);
    let (_, _, space_before, invite_before) = snapshot(&backend).await;

    handle_space_join(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "inviteCode": CODE }),
    )
    .await
    .expect("Space/join");

    let (members, uses, space_after, invite_after) = snapshot(&backend).await;
    assert_eq!(members, vec![ACCOUNT_ID.to_owned()]);
    assert_eq!(uses, 1);
    assert_ne!(space_after, space_before, "Space change not recorded");
    assert_ne!(
        invite_after, invite_before,
        "SpaceInvite change not recorded"
    );
}

/// "... so that concurrent redemptions cannot exceed `maxUses`": the
/// backend re-checks `maxUses` itself rather than trusting the handler's
/// earlier read, and the refusal writes nothing.
#[tokio::test]
async fn exhausted_invite_is_refused_without_any_write() {
    let backend = seeded(1, 1);
    let before = snapshot(&backend).await;

    let refusal = backend
        .join_space_with_invite(
            &(),
            &Id::from(ACCOUNT_ID),
            &Id::from(INVITE_ID),
            &UTCDate::from("2026-01-02T00:00:00Z"),
        )
        .await
        .unwrap();

    assert_eq!(refusal, Err(InviteJoinRefusal::Exhausted));
    assert_eq!(snapshot(&backend).await, before);
}

/// Concurrent redemptions are serialized: exactly one succeeds, `uses`
/// is incremented exactly once and the member appears exactly once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_redemptions_increment_exactly_once() {
    const RACERS: usize = 32;
    let backend = seeded(0, 100);

    let tasks: Vec<_> = (0..RACERS)
        .map(|_| {
            let backend = backend.clone();
            tokio::spawn(async move {
                backend
                    .join_space_with_invite(
                        &(),
                        &Id::from(ACCOUNT_ID),
                        &Id::from(INVITE_ID),
                        &UTCDate::from("2026-01-02T00:00:00Z"),
                    )
                    .await
                    .unwrap()
            })
        })
        .collect();
    let mut outcomes = Vec::with_capacity(RACERS);
    for t in tasks {
        outcomes.push(t.await.unwrap());
    }

    assert_eq!(outcomes.iter().filter(|o| o.is_ok()).count(), 1);
    assert!(
        outcomes
            .iter()
            .all(|o| *o == Ok(()) || *o == Err(InviteJoinRefusal::AlreadyMember))
    );
    let (members, uses, _, _) = snapshot(&backend).await;
    assert_eq!(members, vec![ACCOUNT_ID.to_owned()]);
    assert_eq!(uses, 1);
}
