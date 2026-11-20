# Agent Instructions — jmap-object-uri

The `jmap` URI scheme and JMAP object URIs of
draft-atwood-jmap-object-uri-00 (bd JMAP-l5q7): syntax, the "JMAP Object
URI Types" registry, and resolution against a Session. The normative text
is `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-object-uri-00.md`; cite its
sections in docs and tests.

## Crate family context

```
jmap-types  (JmapUri, JmapUriError, Id, data_types::DATA_TYPES)
    └── jmap-object-uri  ← this crate
```

Scope:

- `JmapUri` and `JmapUriError` stay in `jmap-types`, because type crates
  below this one in the graph (graph-types, chat-types) and
  `jmap-graph-projection-server` use them. This crate re-exports them at
  the root. Syntax fixes go in `crate-jmap-types/src/jmap_uri.rs`.
- `URI_TYPES` holds exactly the registrations the drafts request. When a
  draft adds, renames or drops a row in its "JMAP Object URI Types" IANA
  section, change the table and the hand-copied oracle in
  `tests/spec.rs` together. Each data type must also be a row of
  `jmap_types::data_types::DATA_TYPES`; the tests check it.
- No async, no I/O, no client dependency. Session access is the
  `SessionView` trait; a client crate with a typed Session implements it
  there, not here.

This project uses **bd** (beads) for issue tracking. Run `bd prime` for
full workflow context.

## Build & Test

```bash
cargo test -p jmap-object-uri --all-features
```

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual. If a push is rejected for any reason, stop and ask the human; do NOT `pull --rebase`, merge, force-push or `git reset` (workspace AGENTS.md "Git Commit and Push Policy")
- Tests take their expected values from the drafts' text, examples and IANA tables; never derive an oracle from the code under test.
- Do not use TodoWrite or markdown task lists — use `bd create`
