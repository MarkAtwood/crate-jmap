# Agent Instructions — jmap-vtc-client

## Sibling of the canonical jmap-mail-client — mirror its shape

This crate is a **sibling under the canonical `jmap-mail-client`
extension-client template**. The `JmapVtcExt` extension trait,
`SessionClient` shape, `with_vtc_session` accessor, method signatures,
doc-comment style and test layout mirror `jmap-mail-client`. Differences
are only the spec content (draft-atwood-jmap-vtc-00).

Every VTC method is a standard RFC 8620 §5 method, so the five request
builders live once in `methods/mod.rs` (`standard_get` … `standard_query_changes`)
and each `methods/vtc_*.rs` binds them to a method name and the
`jmap-vtc-types` filter and comparator types.

**The propagation rule** (workspace AGENTS.md "Canonical Templates"):
change `jmap-mail-client` first, then propagate; a canonical change
propagates here in the same pass.

## What This Is

JMAP VTC extension client methods (draft-atwood-jmap-vtc-00 §Methods) on
top of `jmap-base-client`: `VTCCall/*`, `VTCParticipant/*`,
`VTCRecording/*`, `VTCLivestream/*`.

## Build & Test

```bash
cargo fmt --all
cargo clippy -p jmap-vtc-client --all-targets -- -D warnings
cargo test -p jmap-vtc-client
RUSTDOCFLAGS="-D warnings" cargo doc -p jmap-vtc-client --no-deps --all-features
```

## Source Material

- `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-vtc-00.md` — normative for
  method names, arguments and examples (§Methods, §Complete Lifecycle
  Examples)
- RFC 8620 §5 — standard get/changes/set/query/queryChanges shapes

## Restrictions

- Do not use TodoWrite or markdown task lists — use `bd create`
- Tests compare the wire request to the draft's examples, cited by line
