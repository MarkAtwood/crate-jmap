# Agent Instructions — jmap-mdn

Facade for JMAP Message Disposition Notifications (RFC 9007): enables the
`mdn` feature of `jmap-mail-types` and `jmap-mail-server` and re-exports
exactly its items: the wire types as `types` (always) and the
`jmap_mail_server::mdn` items plus `register_mdn_handlers` as `server`
(`server` feature), plus `CAPABILITY`, the one item the 0.0.1 placeholder
published. No `client`: `jmap-mail-client` has no MDN bindings. MDN is a
feature of the mail crates (DECISIONS.md decision 50), not a crate family.
It holds no logic: change the mail crates, not this one. It copies the
shape of `jmap-quotas`, the workspace's first umbrella, and replaces the
0.0.1 name-claim placeholder (bd JMAP-gs76.13).

When the mail crates add or remove an `mdn`-gated item, update the lists
here in the same change.

This project uses **bd** (beads) for issue tracking. Run `bd prime` for
full workflow context.

## Build & Test

```bash
cargo test -p jmap-mdn
cargo test -p jmap-mdn --all-features
```

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual. If a push is rejected for any reason, stop and ask the human; do NOT `pull --rebase`, merge, force-push or `git reset` (workspace AGENTS.md "Git Commit and Push Policy")
- No code beyond re-exports.
- Do not use TodoWrite or markdown task lists — use `bd create`
