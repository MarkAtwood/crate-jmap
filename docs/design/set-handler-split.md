# Design: per-operation sub-functions for `Foo/set` handlers (bd:JMAP-eedw)

Status: implemented in `jmap-mail-server`, the canonical template (decision 54, Mark). The other
extension servers copy this shape when they next touch a long `*_set` handler, or in a sweep.

## 1. Shape

A `Foo/set` handler is a short orchestrator, aiming at under about 80 lines. It does, in order:

1. `extract_account_id`, the `accountExists` check, `enforce_max_objects_in_set`.
2. Any request-wide setup that the checks below need: `old_state`, the `ifInState` check,
   method-specific arguments (`onDestroyRemoveEmails`, `onSuccessActivateScript` and the like),
   snapshots taken before any write.
3. Builds the context struct (§3) and `let mut acc = SetAccumulators::default();`.
4. Calls the sub-functions in the RFC 8620 §5.3 order the handler already used. Between create
   and update, the orchestrator calls `jmap_server::resolve_set_id_refs` itself, so the
   `#creationId` step is visible in one place.
5. Works out `mutated` and ends with `finalize_set_response(...)`.

The refactor does not change behaviour. Error precedence (decision 44: existence and permission
before shape), the order of backend calls, the order of entries in the response, state handling
and wire output all stay the same. A `?` inside a sub-function stops the whole method, as the
inline `?` did before.

## 2. Names

All private to the module:

| Name | What it does |
|---|---|
| `set_create(ctx, args, acc)` | The `create` map. |
| `set_update(ctx, args, acc)` | The `update` map. |
| `set_destroy(ctx, args, acc)` | The `destroy` array, including the whole-request `invalidArguments` for a non-string element. |
| `create_one`, `update_one` | One entry of `create` or `update`, when the loop body would push its function over about 100 lines. They return early where the loop had `continue`. |
| `set_<side effect>` | A method-specific phase after the three: `set_on_success` (EmailSubmission `onSuccess*Email`), `set_activate` (SieveScript activation). |

Each sub-function takes its own key out of `args` (`get` or `remove`, whichever the inline code
used) so that the orchestrator stays uniform. Extra state goes after `acc`, for example Mailbox's
`created_ids` or the email-id map that EmailSubmission keeps for `onSuccess*`. Small checks that
return `Option<Value>` (the SetError value, or `None` to go on) are named after what they find:
`create_conflict`, `create_name_error`, `last_mailbox_removed`, `update_blob_error`.

`mutated` follows what the handler did before. Where it was tracked by hand (Email, Identity,
VacationResponse), each sub-function returns `Result<bool, JmapError>`, meaning that the backend
was written, and the orchestrator does `mutated |= set_update(...).await?;`. `|=` is not
short-circuiting, so every phase still runs. Where it was worked out from the accumulators
(Mailbox, SieveScript, EmailSubmission), the sub-functions return `Result<(), JmapError>` and the
orchestrator computes `!acc.created.is_empty() || ...` at the end. The two are not
interchangeable: a no-op update lands in `updated` without a write.

## 3. Context struct

Each module has its own private struct holding what every sub-function reads:

```rust
/// What every `Foo/set` sub-function needs from the request
/// (docs/design/set-handler-split.md).
struct SetCtx<'a, B: MailBackend> {
    backend: &'a B,
    caller: &'a B::CallerCtx,
    account_id: &'a Id,
    // Module-specific, read-only: created_ids, using, atomic, snapshots, limits ...
}
```

The struct is per module and never `pub`, so it is not public API and each family adds the fields
it needs. It holds only borrowed, read-only request state. Anything a sub-function changes is
passed as `&mut` beside it: `acc: &mut SetAccumulators`, and per-call state such as Mailbox's
`created_ids` clone. When the create loop carries several counters from one entry to the next,
keep them in a second small struct, as SieveScript's `CreateRun` does.

Accumulators are the crate's `SetAccumulators` (helpers.rs), not six loose maps.
`finalize_set_response` then takes `acc` as it is. EmailSubmission uses it too now, and attaches
the implicit `Email/set` with `HandlerOutput::with_extra`. Its implicit `Email/set` body also
goes through `finalize_set_response`, with `mutated = true` so that `newState` is always read
again, as before.

## 4. In jmap-mail-server

| Handler | Before (lines) | After (orchestrator) |
|---|---|---|
| `handle_submission_set` | 408 | 78 |
| `handle_identity_set` | 319 | 53 |
| `handle_email_set` | 299 | 46 |
| `handle_vacation_set` | 221 | 57 |
| `mailbox_set` (behind `handle_mailbox_set`) | 770 | 81 |
| `sieve_set` (behind `handle_sieve_set`) | 665 | 88 |

Each new sub-function is at most 100 lines. One function is still longer:
EmailSubmission's `create_one` (the former `process_create`) has 141 lines. It was 196 before
`derive_envelope` and `check_envelope` were split out of it.

## 5. In jmap-tasks-server

| Handler | Before (lines) | After (orchestrator) |
|---|---|---|
| `handle_task_list_set` | 341 | 54 |
| `handle_task_set` | 288 | 51 |
| `handle_task_notification_set` | 130 | 53 |

Where it differs from the template:

- TaskNotification/set refuses every create and update with `forbidden`, so its `set_create` and
  `set_update` take only `args` and `acc`, cannot fail and return nothing. It never had a
  `resolve_set_id_refs` step (nothing can be created), and still has none.
- TaskList/set checks and parses each create object in `task_list_from_create`, which returns the
  TaskList or the SetError value, as Identity's `identity_builder` does.
- Task/set's per-user routing test is `patch_is_per_user_only`.

## 6. In jmap-calendars-server

| Handler | Before (lines) | After (orchestrator) |
|---|---|---|
| `handle_calendar_event_set` | 429 | 91 |
| `handle_calendar_set` | 424 | 67 |
| `handle_participant_identity_set` | 276 | 59 |
| `handle_calendar_event_notification_set` | 116 | 51 |

Where it differs from the template:

- CalendarEvent/set does not run create, update and destroy one after the other, because the
  §5.9 uid/privacy rule needs the whole request. Its orchestrator keeps the existing order:
  `take_destroys` parses `destroy` first (a non-string element fails the request before
  anything is written); `validate_creates` / `validate_create` and `validate_updates` /
  `validate_update` (with `update_patch_error`) check every entry and return the events and
  patches to apply; the orchestrator calls `privacy_conflicts`; then `set_create`, `set_update`
  and `set_destroy` apply the checked lists, so they take those lists instead of `args`.
  `resolve_set_id_refs` stays between the two validation passes, where it was. `set_destroy`
  cannot fail and returns `bool`. `time_pair_error` is the §5.9 `utcStart`/`start` and
  `utcEnd`/`duration` check that create and update share. The orchestrator is 91 lines, most
  of them comments on the pre-checks and on §5.9.
- Calendar/set and ParticipantIdentity/set end with `set_on_success_default`, the
  `onSuccessSetIsDefault` phase. It cannot fail (backend errors are ignored, as the draft says)
  and returns whether the default changed.
- Calendar/set checks and parses each create object in `calendar_from_create`, which calls
  `fill_create_defaults` for the server-set and defaulted properties. The `calendarHasEvent`
  check and the `onDestroyRemoveEvents` cleanup are `destroy_events_error`. Its update runs the
  metadata, i18n and `shareWith` checks as one `and_then` chain, in the same order, so the
  first failing check still decides the SetError.
- CalendarEventNotification/set is destroy-only, like TaskNotification/set (§5).

## 7. In jmap-contacts-server

| Handler | Before (lines) | After (orchestrator) |
|---|---|---|
| `handle_address_book_set` | 502 | 75 |
| `handle_contact_card_set` | 272 | 56 |

Where it differs from the template:

- AddressBook/set has two phases after destroy: `set_on_success_default` (the RFC 9610 §2.3
  `onSuccessSetIsDefault` patch) and `set_report_demotions` (books that the single-default
  rule demoted, bd:JMAP-qz9v.57). Both return `Result<bool, JmapError>`. The pre-checks use
  `take_on_success_set_is_default` and `default_book_ids`. Create objects are checked and
  parsed in `address_book_from_create`, and the `addressBookHasContents` check and the
  `onDestroyRemoveContents` cascade are `destroy_contents_error`. Its update runs the
  metadata, i18n and `shareWith` checks as one `and_then` chain, as Calendar/set does.
- ContactCard/set has an `update_one`.

## 8. In the sibling servers

The same shape, with these differences per crate. Sizes are before → after for the
orchestrator, in lines.

- **jmap-sharing-server** (`handle_principal_set` 302 → 61, `handle_share_notification_set`
  131 → 55). Principal/set has `principal_from_create` (check and parse, like Identity's
  `identity_builder`) and `update_one`. ShareNotification/set refuses create and update with
  `forbidden` without touching the backend, so its `set_create` and `set_update` take no `ctx`
  and return nothing. Only `set_destroy` reports a write.
- **jmap-scene-server** (`handle_scene_object_set` 123 → 48, `handle_scene_region_set` 117 → 46,
  `scene_asset_set` 109 → 47, `handle_scene_avatar_set` 106 → 46). The per-entry functions
  already existed under their own names (`create_scene_object`, `update_region`, `enter_region`,
  `update_avatar`, `create_asset` and so on) and keep them: they are these modules' `create_one`
  and `update_one`. SceneObject clones `created_ids` and passes it to `set_create` as `&mut` and
  to `set_update` as `&`, as Mailbox does. SceneRegion and SceneAvatar only read it, so it sits in
  `SetCtx`. SceneAvatar's `set_destroy` refuses every id without touching the backend, so it
  takes no `ctx` and does not feed `mutated`. New spec features go into `set_create` /
  `set_update` or the per-entry functions, not the orchestrators. `update_avatar` (180 lines)
  is still over 100.
- **jmap-vtc-server** (`handle_vtc_participant_set` 122 → 53, `handle_vtc_call_set` 121 → 47,
  the media `handle_set` behind VTCRecording/set and VTCLivestream/set 135 → 51). Same as
  jmap-scene-server: the existing per-entry functions keep their names, VTCCall passes its
  `created_ids` clone beside `acc` like SceneObject, and the others keep `created_ids` in
  `SetCtx`. VTCParticipant's `set_destroy` only reads (`forbidden` or `notFound`), so it returns
  `Result<(), JmapError>` and does not feed `mutated`. The media driver serves two types:
  `SetCtx` carries the `Kind`, and `set_destroy` is generic over the object type. Per-entry
  functions still over 100 lines: `update_participant` (300), `create_call` (179),
  `update_call` (157), `update_recording` (138), `create_livestream` (125),
  `create_participant` (109).
- **jmap-chat-mimi-server** (`handle_key_package_set` 152 → 44). The crate had no
  `SetAccumulators`. A private `helpers.rs` now holds `SetAccumulators` and
  `finalize_set_response`, copied from jmap-scene-server. The wire output does not change:
  `updated` is always empty, so it is `null` as before. `set_update` (every entry `forbidden`)
  takes no `ctx`.
- **jmap-filenode-server** (`filenode_set` behind `handle_filenode_set` 1151 → 99). `mutated`
  was tracked by hand, but every place that set it also pushed to `created`, `updated` or
  `destroyed` (a destroy, an `onExists` replacement, or a destroy run early to free a name), and
  no other place did. So the split works it out from the accumulators, as Mailbox does, and the
  sub-functions return `Result<(), JmapError>`. Create and update had two copies of the
  sibling-name and `onExists` logic. Both now call `name_collision` (with `on_exists_outcome`),
  which takes the updated node's own id so that the node does not collide with itself under case
  folding. The other helpers are `parse_on_exists`, `DestroyPlan::new`, `node_from_create`,
  `create_shape_error`, `create_store_error` (blob, depth, parent), `parse_update`,
  `update_stored_node`, `stored_node_error`, `typed_patch_properties`, `update_parent_error`,
  `move_depth_error`, `update_collision` and `update_sha256`. Still over 100 lines:
  `destroy_one` (129), which existed before, and `filenode_copy` (795), which is FileNode/copy,
  not a `/set`.
- **jmap-graph-server** (`handle_set` in helpers.rs, the driver behind every Graph `/set`,
  128 → 50). The driver is generic over the `SetOps` trait, so the sub-functions are generic
  over `S: SetOps<B>`. Like jmap-chat-mimi-server, the crate had no `SetAccumulators`, and
  helpers.rs now has it and `finalize_set_response`. `mutated` was already worked out from the
  maps.
- **Checked, no `*_set` over 100 lines:** jmap-blob-server (`handle_blob_set` 84; its
  `create_one`, 157, is the Blob/upload create steps, already outside the handler),
  jmap-object-history-server, jmap-quotas-server, jmap-metadata-server,
  jmap-graph-projection-server, jmap-chat-scene-server, jmap-scene-vtc-server,
  jmap-chat-vtc-server, jmap-chat-filenode-server, jmap-chat-federation-server.
