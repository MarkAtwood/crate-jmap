# Agent Instructions — jmap-search

Reserved umbrella for the planned JMAP search extension (bd JMAP-nn3i): a
generic `where` filter language for every type's `/query`. It has no
public items and no dependencies yet, because `jmap-search-types`,
`-client` and `-server` do not exist in this workspace. It replaces the
0.0.1 name-claim placeholder (bd JMAP-gs76).

When the component crates land, give this crate the `jmap-quotas` shape:
`types` always, `client` and `server` behind features (and `memory` if the
server has a reference backend), a `tests/features.rs`, and an updated
README. Do not re-export the Graph `search` modules from here: their API
is Graph-specific.

This project uses **bd** (beads) for issue tracking. Run `bd prime` for
full workflow context.

## Build & Test

```bash
cargo test -p jmap-search
```

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual. If a push is rejected for any reason, stop and ask the human; do NOT `pull --rebase`, merge, force-push or `git reset` (workspace AGENTS.md "Git Commit and Push Policy")
- No code beyond re-exports.
- Do not use TodoWrite or markdown task lists — use `bd create`
