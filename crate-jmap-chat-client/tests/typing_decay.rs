//! Client typing-indicator decay timer.
//!
//! Oracle: draft-atwood-jmap-chat-00.md L1094 (jmap-chat-spec HEAD), §Chat/typing:
//! "Clients SHOULD use a decay timer to recover from any stale typing state;
//! if no typing event is received for a given `(chatId, senderId)` pair
//! within approximately 10 seconds, the client SHOULD hide the typing
//! indicator for that pair."
//!
//! L1077: "`typing` (Boolean): `true` if the user has started typing; `false`
//! if typing has stopped."
//!
//! L1096: "with one `Chat/typing` call accepted per 3 seconds while a sender
//! is actively typing, 10 seconds represents three missed events".

use std::time::{Duration, Instant};

use jmap_chat_client::{TYPING_DECAY, TypingIndicators};
use jmap_types::Id;

fn ids() -> (Id, Id) {
    (Id::from("chat-1"), Id::from("alice"))
}

/// L1094: "within approximately 10 seconds".
#[test]
fn decay_window_is_ten_seconds() {
    assert_eq!(TYPING_DECAY, Duration::from_secs(10));
}

/// L1094: visible before 10 s without a refresh, hidden after.
#[test]
fn indicator_visible_before_and_hidden_after_ten_seconds() {
    let (chat, alice) = ids();
    let t0 = Instant::now();
    let mut typing = TypingIndicators::new();
    typing.record(&chat, &alice, true, t0);

    assert!(typing.is_typing(&chat, &alice, t0));
    assert!(typing.is_typing(&chat, &alice, t0 + Duration::from_millis(9_999)));
    assert!(!typing.is_typing(&chat, &alice, t0 + Duration::from_secs(10)));
    assert!(!typing.is_typing(&chat, &alice, t0 + Duration::from_secs(11)));
    assert_eq!(
        typing
            .typing_in(&chat, t0 + Duration::from_secs(11))
            .count(),
        0
    );
}

/// L1096: a sender typing steadily sends one event per 3 s; each event
/// restarts the timer for that pair, so the indicator stays visible.
#[test]
fn refresh_every_three_seconds_keeps_indicator_visible() {
    let (chat, alice) = ids();
    let t0 = Instant::now();
    let mut typing = TypingIndicators::new();
    for i in 0..5 {
        typing.record(&chat, &alice, true, t0 + Duration::from_secs(3 * i));
    }
    // Last event at 12 s; 21 s is 9 s later.
    assert!(typing.is_typing(&chat, &alice, t0 + Duration::from_secs(21)));
    assert!(!typing.is_typing(&chat, &alice, t0 + Duration::from_secs(22)));
}

/// L1077: `typing: false` means typing has stopped; hide at once.
#[test]
fn typing_false_hides_immediately() {
    let (chat, alice) = ids();
    let t0 = Instant::now();
    let mut typing = TypingIndicators::new();
    typing.record(&chat, &alice, true, t0);
    typing.record(&chat, &alice, false, t0 + Duration::from_secs(1));
    assert!(!typing.is_typing(&chat, &alice, t0 + Duration::from_secs(1)));
}

/// L1094: the timer is per `(chatId, senderId)` pair.
#[test]
fn timer_is_per_chat_and_sender_pair() {
    let (chat, alice) = ids();
    let bob = Id::from("bob");
    let other_chat = Id::from("chat-2");
    let t0 = Instant::now();
    let mut typing = TypingIndicators::new();
    typing.record(&chat, &alice, true, t0);
    typing.record(&chat, &bob, true, t0 + Duration::from_secs(6));
    typing.record(&other_chat, &alice, true, t0 + Duration::from_secs(6));

    let at = t0 + Duration::from_secs(12);
    assert!(!typing.is_typing(&chat, &alice, at));
    assert!(typing.is_typing(&chat, &bob, at));
    assert!(typing.is_typing(&other_chat, &alice, at));
    let in_chat: Vec<&Id> = typing.typing_in(&chat, at).collect();
    assert_eq!(in_chat, vec![&bob]);
}
