# jmap-tasks-types

Serde-annotated Rust types for the JMAP Tasks extension ([draft-ietf-jmap-tasks-06]) and
JSCalendar Task subset ([RFC 8984]). Types only — no method handlers, no async, no network I/O.

## What it is

| Type | Description |
|---|---|
| `TaskList` | Task list object (§3) |
| `TaskRights` | Per-user rights on a task list (`myRights`) |
| `TaskListRole` | Well-known task list roles (`inbox`, `outbox`, etc.) |
| `Task` | Task object (§4) — JSCalendar Task subset |
| `TaskProgress` | Task progress status (`needs-action`, `in-process`, `completed`, `failed`, `cancelled`) |
| `Checklist` | Named checklist attached to a Task |
| `CheckItem` | Individual item within a Checklist |
| `Comment` | Comment attached to a Task |
| `Person` | A person reference (name, email, kind) used in assignees and changed-by |
| `TaskNotification` | Server-generated notification about a task change (§5) |
| `NotificationType` | Enum of `TaskNotification` change kinds |
| `TaskFilterCondition` | Filter arguments for `Task/query` (§4.13) |
| `TaskNotificationFilterCondition` | Filter arguments for `TaskNotification/query` |
| `TaskComparator` | Sort comparator for `Task/query` |
| `TaskNotificationComparator` | Sort comparator for `TaskNotification/query` |
| `TaskProperty` | Enum of legal `Task` property names for `properties` arrays |
| `TaskListProperty` | Enum of legal `TaskList` property names |
| `TaskNotificationProperty` | Enum of legal `TaskNotification` property names |
| `TasksCapability` | Session-level capability object (empty `{}`) |
| `TasksAccountCapability` | Account-level capability (minDateTime, maxDateTime, mayCreateTaskList) |
| `TasksRecurrencesCapability` / `TasksRecurrencesAccountCapability` | Recurrences extension (§1.6.2) |
| `TasksAssigneesCapability` / `TasksAssigneesAccountCapability` | Assignees extension (§1.6.3) |
| `TasksAlertsCapability` / `TasksAlertsAccountCapability` | Alerts extension (§1.6.4) |
| `TasksMultilingualCapability` / `TasksMultilingualAccountCapability` | Multilingual extension (§1.6.5) |
| `TasksCustomTimeZonesCapability` / `TasksCustomTimeZonesAccountCapability` | Custom time zones extension (§1.6.6) |
| `PrincipalTasksCapability` | Per-principal Tasks capability |

Capability URI constants:

| Constant | Value |
|---|---|
| `JMAP_TASKS_URI` | `"urn:ietf:params:jmap:tasks"` |
| `JMAP_TASKS_RECURRENCES_URI` | `"urn:ietf:params:jmap:tasks:recurrences"` |
| `JMAP_TASKS_ASSIGNEES_URI` | `"urn:ietf:params:jmap:tasks:assignees"` |
| `JMAP_TASKS_ALERTS_URI` | `"urn:ietf:params:jmap:tasks:alerts"` |
| `JMAP_TASKS_MULTILINGUAL_URI` | `"urn:ietf:params:jmap:tasks:multilingual"` |
| `JMAP_TASKS_CUSTOMTIMEZONES_URI` | `"urn:ietf:params:jmap:tasks:customtimezones"` |

## What it's for

draft-ietf-jmap-tasks data types, consumed by `jmap-tasks-server`
(method handlers + the `TasksBackend` trait) and `jmap-tasks-client`
(typed method bindings). Planned to consume `jmap-jscalendar-types` for
shared RFC 8984 JSCalendar sub-objects alongside `jmap-calendars-types`.
Sibling to `jmap-mail-types`, `jmap-contacts-types`, and
`jmap-calendars-types` in the workspace's extension-types family.

## Filter extensibility

Filter and comparator types in this crate — `TaskFilterCondition`,
`TaskNotificationFilterCondition`, `TaskComparator`,
`TaskNotificationComparator`, and the generic `Filter<T>` / `Operator`
re-exported from `jmap-types` — are **intentionally not extensible** via
vendor "extras" fields. A filter clause the server does not understand
silently breaks query correctness: the client gets the wrong set of records
back with no error signal. So these types deliberately have no `extra`
catch-all field.

Vendors who need to filter on custom fields have two options:

- **IETF-track (recommended).** Use the JMAP Object Metadata extension
  (`draft-ietf-jmap-metadata`, capability URI `urn:ietf:params:jmap:metadata`),
  which defines a `Metadata` / `Annotation` companion object keyed by
  `(relatedType, relatedId)` with capability-declared schema (`metadataTypes`
  / `maxDepth`) and a `Metadata/query` `textMatch` filter. This is the
  workspace's recommended path for vendor data that needs to be queryable.
  Implemented in [`jmap-metadata-types`](../crate-jmap-metadata-types),
  [`jmap-metadata-server`](../crate-jmap-metadata-server), and
  [`jmap-metadata-client`](../crate-jmap-metadata-client) (bd JMAP-06zp).
- **Pre-IETF escape.** If you cannot wait for the metadata draft, escape the
  filter tree to `serde_json::Value` or fork the `FilterCondition` types.
  See
  [`crate-jmap-calendars-types/PLAN.md`](../crate-jmap-calendars-types/PLAN.md)
  for the hybrid sloppy-value pattern.

This policy is part of the workspace extras-preservation policy documented in
the workspace [`AGENTS.md`](../AGENTS.md); the filter-algebra exclusion
decision is bd JMAP-lbdy.

## Spec coverage

**draft-ietf-jmap-tasks-06 sections implemented:**

- §1.6 — Capability URIs (all six)
- §3 — TaskList object and rights
- §3.4 — `isSubscribed` field (used in place of the spec's ambiguous §3.4 `isVisible` reference)
- §4 — Task object (JSCalendar Task subset, including `isDraft`, `utcStart`, `utcDue`)
- §4.5.1 — Per-user Task properties (`keywords`, `color`, `freeBusyStatus`, `useDefaultAlerts`, `alerts`)
- §4.12 — `Checklist` and `CheckItem`
- §4.13 — `TaskFilterCondition` (excluding `filterAsTree`, which is a TODO in the spec itself)
- §5 — `TaskNotification` and `NotificationType`
- §5.5 — `TaskNotificationFilterCondition`

**RFC 8984 sections implemented:**

- §4.1 — `Task` base properties inherited from JSCalendar (`uid`, `title`, `description`, `start`, `due`, `timeZone`, `showWithoutTime`, `estimatedDuration`, `priority`, `privacy`, `color`, `keywords`)
- §4.4 — `Comment` and `Person` types

## How to use

```rust
use jmap_tasks_types::{TaskList, Task};

// Deserialize a TaskList from a JMAP response.
let task_list: TaskList = serde_json::from_str(r#"{
    "id": "list1",
    "name": "Work",
    "sortOrder": 0,
    "isSubscribed": true,
    "myRights": {
        "mayReadItems": true,
        "mayWriteAll": true,
        "mayWriteOwn": true,
        "mayUpdatePrivate": true,
        "mayRSVP": false,
        "mayAdmin": false,
        "mayDelete": false
    }
}"#)?;

// Deserialize a Task.
let task: Task = serde_json::from_str(r#"{
    "id": "task1",
    "taskListId": "list1",
    "title": "Write quarterly report",
    "start": "2025-06-01T09:00:00",
    "due": "2025-06-15T17:00:00",
    "timeZone": "America/New_York",
    "isDraft": false
}"#)?;
# Ok::<(), serde_json::Error>(())
```

## How it works

All structs carry `#[serde(rename_all = "camelCase")]` to produce camelCase JSON field names
as required by the JMAP wire format. Extension capability structs use `#[non_exhaustive]` so
that adding new optional fields in a future draft revision is not a breaking change.

The `TaskListRole` and `TaskProgress` types are string-backed enums with an `Other(String)`
fallback variant so that unrecognised server values round-trip without data loss.

## Gotchas

- The draft expired in November 2023 and has not been updated; some fields use best-judgment
  interpretation where the spec is ambiguous or contradicts itself. In particular, `isVisible`
  is referenced in §3.4 without being defined in §3; this crate follows the normative §3 field
  list which uses `isSubscribed`.
- `filterAsTree` for `Task/query` is mentioned in §4.13 as a TODO in the spec itself; it is
  not implemented.
- `workflowStatuses` on `TaskList` are user-defined strings; no validation against the list
  is performed at the type layer.
- The spec typos `assigneee` (three e's) in §4.2.3 and `TaskId` (capital T) in §5.1 are
  corrected silently; wire names are `assignee` and `taskId` respectively.

## References

- **[draft-ietf-jmap-tasks-06]** — JMAP Tasks (normative for all type definitions)
- **[RFC 8984]** — JSCalendar (Task property subset)
- **[RFC 8620]** — JMAP Core (Id, State, SetError, request/response shape)

[draft-ietf-jmap-tasks-06]: https://www.ietf.org/archive/id/draft-ietf-jmap-tasks-06.txt
[RFC 8984]: https://www.rfc-editor.org/rfc/rfc8984
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
