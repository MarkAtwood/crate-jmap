# Agent Instructions — jmap-search

The `urn:ietf:params:jmap:search` core (bd JMAP-nn3i, decision 51): the
capability object, the `where` expression AST and its checks, and
(feature `eval`) the evaluator. The normative text is the local draft
`docs/specs/jmap-search.md` (draft-atwood-jmap-search-00) at the
workspace root; cite its sections in docs and tests.

Scope:

- Type-neutral only. No Graph structural operators (`$edge`,
  `$reachable`, `$from`, `$to`), no `GraphView`: those stay in
  `jmap-graph-types` / `jmap-graph-server`, which build on this crate.
  Changing `FieldTest`, `Bound`, `WhereError`, `FieldTest::matches` or
  `collate` changes Graph search too; run the graph crates' tests.
- No async, no I/O, no backend trait. Each family's server crate owns its
  `search_capability` hook and its candidate view (Mail: Email
  `/get` form with `$text` = RFC 8621 `text`; Chat: stored Message).
- `eval` stays an optional feature so type and client crates do not pull
  in the ICU data.

It is a single crate for now, not the `jmap-quotas` umbrella shape: split
into `-types`/`-server` only if a client-only consumer needs it without
`jmap-types` (bd JMAP-8xwm, JMAP-hfom, JMAP-k42x track the split).

This project uses **bd** (beads) for issue tracking. Run `bd prime` for
full workflow context.

## Build & Test

```bash
cargo test -p jmap-search --all-features
```

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual. If a push is rejected for any reason, stop and ask the human; do NOT `pull --rebase`, merge, force-push or `git reset` (workspace AGENTS.md "Git Commit and Push Policy")
- Tests quote docs/specs/jmap-search.md by section; never derive an oracle from the code under test.
- Do not use TodoWrite or markdown task lists — use `bd create`
