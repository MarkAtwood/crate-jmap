//! The `testkit` conformance suite run on the reference `MemoryBackend`,
//! once per `supportsThreads` setting so both sides of the thread-count
//! gate execute.

use jmap_chat_server::ChatLimits;
use jmap_chat_server::memory::MemoryBackend;

mod default_limits {
    use super::MemoryBackend;

    jmap_chat_server::chat_backend_conformance!(MemoryBackend::new);
}

mod threads {
    use super::{ChatLimits, MemoryBackend};

    jmap_chat_server::chat_backend_conformance!(|| {
        let backend = MemoryBackend::new();
        backend.set_limits_for_test(Some(ChatLimits::default().with_supports_threads(true)));
        backend
    });
}
