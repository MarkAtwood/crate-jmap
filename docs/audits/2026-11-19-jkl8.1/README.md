# Tasks: Mark's 17 jmapio/jmap items against the crates (bd JMAP-jkl8.1)

Spec base: draft-ietf-jmap-tasks-06 (editor's source at cd67de0). Items:
github.com/jmapio/jmap #456-#472, all open, read 2026-11-19. Crates:
jmap-tasks-types, jmap-tasks-server, jmap-tasks-client at crate-jmap
48d97d06. Prior audit: JMAP-t8ej.17.

| Item | What it changes | Wire or text | Crate state | Bead |
|---|---|---|---|---|
| #456 (issue) | Defines Task/get, /set, /query and a Task FilterCondition: `inTaskLists` (Id[]), `after`/`before` on due (and start), `progress`, `workflowStatus`, `text`/`title`; Task/get `timeZone`; Task/query `expandRecurrences` | Wire | `TaskFilterCondition` (deny_unknown_fields) has a single `taskListId`, `uid`, `hasKeyword`, `notKeyword`, `text`, `title`, `description`, `after`/`before` on due only, `isDraft`, `progress`. Missing: `inTaskLists`, `workflowStatus`, start in after/before, Task/get `timeZone` (compute_utc_times gets `None`), `expandRecurrences`. The issue says the crate "ended up with" `inTaskLists`; it has `taskListId` | jkl8.2 |
| #457 (issue), #458 (PR) | `TaskId: String` → `taskId: Id`; CheckItem `assigneee` → `assignee` | Wire | Already `taskId: Id` and `assignee`; the misspellings are not accepted on input | jkl8.3 |
| #459 (issue), #460 (PR) | SetError prose `TaskListHasTask` → `taskListHasTask` | Wire (one spelling) | Already `taskListHasTask` | jkl8.3 |
| #461 (issue), #462 (PR) | Task owner as Calendars: `participants` with the `owner` role; no owner when none has it | Wire (who may edit under mayWriteOwn) | No owner definition: the `mayWriteOwn` contract (tasks-server backend.rs) does not say who the owner is; no helper | jkl8.4 |
| #463 (issue), #464 (PR) | `utcStart`/`utcDue` from `start`/`due` converted to UTC, `UTCDate|null` | Wire | Computed from `start`/`due` (not a "duration"); but when absent the key is omitted, not `null` (tasks-server task.rs, Task/get). The issue says the crate returns null; it omits | jkl8.5 |
| #465 (issue) | Rule for `workflowStatus` vs `progress`: both set, value not in the list's `workflowStatuses`, mirroring into `progress`, list changes | Wire | No check: `workflowStatus` is a free `Option<String>`; the "must be in workflowStatuses" and "progress MUST be null" doc comments are not enforced | jkl8.6 |
| #466 (issue), #467 (PR) | IANA: register checklists, comments, Checklist/CheckItem/Comment/Person, Relation values `depends-on`/`clone`/`duplicate`/`cause`, role `assignee`; fix tasks:assignees naming tasks:recurrences | Text (registry) | Nothing to change: roles and relation types are open string maps in jmap-jscalendar-types, so the values are accepted | jkl8.9 (draft) |
| #468 (issue), #469 (PR) | `mayUpdatePrivate` covers the per-user properties (§4.5.1); drop `isVisible` | Wire (what a mayUpdatePrivate-only user may patch) | `PER_USER_TASK_PROPERTIES` = §4.5.1's list (keywords, color, freeBusyStatus, useDefaultAlerts, alerts); per-user-only patches go to `update_task_per_user`, whose contract is mayUpdatePrivate; no `isVisible`. Gap: the memory backend is single-user (CallerCtx `()`), so no test exercises a mayUpdatePrivate-only sharee | jkl8.7 |
| #470 (issue) | Participant matching by Calendars' `calendarAddress`, not `sendTo`/`scheduleId` | Wire | `PrincipalTasksCapability` still has `sendTo`; Person.uri doc says scheduleId | jkl8.8 |
| #471 (issue), #472 (PR) | Editorial: `UnignedInt`, "MUST null", trash "messages", RFC 8984 section and citation, stray line | Text (`UnignedInt` would be registered as written) | Nothing to change: `estimatedWork` is an unsigned integer | jkl8.9 (draft) |

Seventeen numbers: 10 issues (#456, #457, #459, #461, #463, #465, #466,
#468, #470, #471) and 7 PRs (#458, #460, #462, #464, #467, #469, #472).

## Corrections to the issue texts

- #456 says the crate's filter has `inTaskLists`; it has a single
  `taskListId`. The proposal, not the crate, is the target.
- #463 says the crate returns `null` for an absent due; it omits the key.

Both were noted in JMAP-t8ej.17. A later comment on each issue would fix
the record; that is outward-facing and Mark's call.

## Open question for Mark

#465 leaves one choice open: when `workflowStatus` is one of the RFC 8984
progress values, either the server also sets `progress` to it, or the
"`progress` MUST be null" rule is dropped. jkl8.6 needs that answer.
