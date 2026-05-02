# jmap-tasks-types — Implementation Plan

Data types for the JMAP Tasks extension (draft-ietf-jmap-tasks-06). Types only —
no method handlers, no async, no network I/O. This crate sits between `jmap-types`
(shared JMAP base primitives) and `jmap-tasks-server` / `jmap-tasks-client`
(method handlers and client extension trait).

## Crate Family Position

```
jmap-types (RFC 8620 wire primitives)
    └── jmap-tasks-types  ← this crate
            ├── jmap-tasks-server (method handlers)
            └── jmap-tasks-client (client extension trait)
```

## What This Crate Is

Serde-serializable Rust types for every object defined in draft-ietf-jmap-tasks-06
and the JSCalendar Task subset of RFC 8984. One source module per object type.

## What This Crate Is Not

- Not a method handler — no `Task/get`, `TaskList/set`, etc. logic lives here
- Not async — no tokio, no futures, no network I/O
- Not a CalDAV adapter — we do not handle iCalendar VTODO conversion here
- Not a full JSCalendar implementation — only the Task-relevant properties

## Source Material

### Normative

- `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-tasks-06.txt` — JMAP Tasks
  binding: TaskList, Task (JMAP-specific additions), TaskNotification, rights,
  capabilities, and method definitions. Read the relevant section before naming
  any field.
- `~/PROJECT/jmap-chat-spec/references/rfc8984.txt` — JSCalendar (RFC 8984).
  The Task wire format is JSCalendar Task. Read §4 (common properties) and §5.2
  (Task-specific properties) for every field in `Task`.
- `~/PROJECT/jmap-chat-spec/references/rfc8620.txt` — JMAP base. Filter,
  Comparator, session types, Id, State, UTCDate.

### Draft caveat

draft-ietf-jmap-tasks-06 expired in September 2023. It is an early pre-RFC
draft. Several sections are explicitly marked TODO (e.g. Task/get, Task/set,
Task/query descriptions are stubs that say "copy from Calendars"). The spec
deliberately mirrors JMAP Calendars (draft-ietf-jmap-calendars-10) wherever
possible. Where the Tasks draft is silent, treat the Calendars draft as
informative context, but do not copy fields that the Tasks draft does not
explicitly include.

The capability URI `urn:ietf:params:jmap:tasks` is registered in §7.1 of the
Tasks draft. The five sub-capability URIs for optional extensions are in §§7.2–7.6.

## Full Type Table

Each object type maps to one source module. The normative reference column cites
the exact draft section or RFC section that defines each field.

| Module | Type(s) | Normative reference |
|---|---|---|
| `task_list.rs` | `TaskList`, `TaskRights`, `TaskListRole` | draft-tasks-06 §3 |
| `task.rs` | `Task`, `TaskProgress`, `Checklist`, `CheckItem`, `Comment`, `Person` | draft-tasks-06 §4; RFC 8984 §4, §5.2 |
| `notification.rs` | `TaskNotification`, `NotificationType` | draft-tasks-06 §5 |
| `filter.rs` | `TaskFilterCondition`, `TaskNotificationFilterCondition`, `TaskComparator` | draft-tasks-06 §4.13, §5.5 |
| `capability.rs` | `TasksCapability`, `TasksAccountCapability` | draft-tasks-06 §1.6 |

Generic query types (`Filter<T>`, `FilterOperator<T>`, `Operator`) live in
`jmap-types::query` (RFC 8620 §5.5).

### TaskList (draft-tasks-06 §3)

All fields as they appear on the wire (camelCase):

| Rust field | Wire name | Type | Notes |
|---|---|---|---|
| `id` | `id` | `Id` | immutable; server-set |
| `role` | `role` | `Option<TaskListRole>` | null \| "inbox" \| "trash" |
| `name` | `name` | `String` | min 1 char, max 255 bytes UTF-8 |
| `description` | `description` | `Option<String>` | default null |
| `color` | `color` | `Option<String>` | CSS color name or #rrggbb |
| `keyword_colors` | `keywordColors` | `Option<HashMap<String, String>>` | default null |
| `category_colors` | `categoryColors` | `Option<HashMap<String, String>>` | default null |
| `sort_order` | `sortOrder` | `u32` | default 0; 0 ≤ n < 2^31 |
| `is_subscribed` | `isSubscribed` | `bool` | per-user preference |
| `time_zone` | `timeZone` | `Option<String>` | IANA TZ id or null |
| `workflow_statuses` | `workflowStatuses` | `Vec<String>` | default: six values from RFC 8984 §5.2.5 plus "pending" |
| `share_with` | `shareWith` | `Option<HashMap<Id, TaskRights>>` | null = not shared |
| `my_rights` | `myRights` | `TaskRights` | server-set |
| `default_alerts_with_time` | `defaultAlertsWithTime` | `Option<HashMap<Id, serde_json::Value>>` | alerts extension only; opaque Alert objects (RFC 8984 §4.5.2) |
| `default_alerts_without_time` | `defaultAlertsWithoutTime` | `Option<HashMap<Id, serde_json::Value>>` | alerts extension only |

`TaskRights` (draft-tasks-06 §3):

| Rust field | Wire name | Type |
|---|---|---|
| `may_read_items` | `mayReadItems` | `bool` |
| `may_write_all` | `mayWriteAll` | `bool` |
| `may_write_own` | `mayWriteOwn` | `bool` |
| `may_update_private` | `mayUpdatePrivate` | `bool` |
| `may_rsvp` | `mayRSVP` | `bool` |
| `may_admin` | `mayAdmin` | `bool` |
| `may_delete` | `mayDelete` | `bool` |

`TaskListRole` enum: `Inbox`, `Trash`. Custom `Display`/serde for `"inbox"` /
`"trash"` wire values; an `Other(String)` variant handles future extensions
gracefully (mirrors the MailboxRole pattern).

### Task (draft-tasks-06 §4; RFC 8984 §4 and §5.2)

Task is a JSCalendar Task object with JMAP-specific additions. The core JSCalendar
properties come from RFC 8984 §4 (common) and §5.2 (Task-specific). JMAP adds
the `id`, `taskListId`, `isDraft`, `utcStart`, `utcDue`, `sortOrder`, and
`workflowStatus` properties (draft-tasks-06 §4).

All fields are `Option` because RFC 8620 §5.1 allows partial responses (clients
request only the properties they need via the `properties` argument to `Task/get`).
A field absent from the server response must not fail deserialization.

**JMAP-added fields** (draft-tasks-06 §4):

| Rust field | Wire name | Type | Notes |
|---|---|---|---|
| `id` | `id` | `Option<Id>` | immutable; server-set |
| `task_list_id` | `taskListId` | `Option<Id>` | exactly one TaskList |
| `is_draft` | `isDraft` | `Option<bool>` | default false; once false cannot revert |
| `utc_start` | `utcStart` | `Option<String>` | UTCDate; server-calculated; not in default response |
| `utc_due` | `utcDue` | `Option<String>` | UTCDate; server-calculated; not in default response |
| `sort_order` | `sortOrder` | `Option<u32>` | default 0 |
| `workflow_status` | `workflowStatus` | `Option<String>` | null or one of TaskList.workflowStatuses |
| `base_task_id` | `baseTaskId` | `Option<Id>` | recurrences extension; server-set for expanded instances |
| `is_origin` | `isOrigin` | `Option<bool>` | assignees extension; server-set |
| `may_invite_self` | `mayInviteSelf` | `Option<bool>` | assignees extension; default false |
| `may_invite_others` | `mayInviteOthers` | `Option<bool>` | assignees extension; default false |
| `hide_attendees` | `hideAttendees` | `Option<bool>` | assignees extension; default false |
| `estimated_work` | `estimatedWork` | `Option<u64>` | Tasks-defined JSCalendar property; story points |
| `impact` | `impact` | `Option<String>` | Tasks-defined JSCalendar property |
| `checklists` | `checklists` | `Option<HashMap<Id, Checklist>>` | Tasks-defined JSCalendar property |
| `comments` | `comments` | `Option<HashMap<Id, Comment>>` | Tasks-defined JSCalendar property |

**JSCalendar Metadata Properties** (RFC 8984 §4.1):

| Rust field | Wire name | Type |
|---|---|---|
| `at_type` | `@type` | `Option<String>` | always "Task" on wire |
| `uid` | `uid` | `Option<String>` |
| `related_to` | `relatedTo` | `Option<HashMap<String, serde_json::Value>>` |
| `prod_id` | `prodId` | `Option<String>` |
| `created` | `created` | `Option<String>` | UTCDateTime |
| `updated` | `updated` | `Option<String>` | UTCDateTime |
| `sequence` | `sequence` | `Option<u64>` |

**JSCalendar What/Where Properties** (RFC 8984 §4.2):

| Rust field | Wire name | Type |
|---|---|---|
| `title` | `title` | `Option<String>` |
| `description` | `description` | `Option<String>` |
| `description_content_type` | `descriptionContentType` | `Option<String>` |
| `show_without_time` | `showWithoutTime` | `Option<bool>` |
| `locations` | `locations` | `Option<HashMap<Id, serde_json::Value>>` |
| `virtual_locations` | `virtualLocations` | `Option<HashMap<Id, serde_json::Value>>` |
| `links` | `links` | `Option<HashMap<Id, serde_json::Value>>` |
| `locale` | `locale` | `Option<String>` |
| `keywords` | `keywords` | `Option<HashMap<String, bool>>` | per-user in shared lists |
| `categories` | `categories` | `Option<HashMap<String, bool>>` |
| `color` | `color` | `Option<String>` | per-user in shared lists |

**JSCalendar Recurrence Properties** (RFC 8984 §4.3, gated on recurrences extension):

| Rust field | Wire name | Type |
|---|---|---|
| `recurrence_id` | `recurrenceId` | `Option<String>` | LocalDateTime |
| `recurrence_id_time_zone` | `recurrenceIdTimeZone` | `Option<String>` |
| `recurrence_rules` | `recurrenceRules` | `Option<Vec<serde_json::Value>>` |
| `excluded_recurrence_rules` | `excludedRecurrenceRules` | `Option<Vec<serde_json::Value>>` |
| `recurrence_overrides` | `recurrenceOverrides` | `Option<HashMap<String, serde_json::Value>>` |
| `excluded` | `excluded` | `Option<bool>` |

**JSCalendar Sharing/Scheduling Properties** (RFC 8984 §4.4, gated on assignees extension for participants):

| Rust field | Wire name | Type |
|---|---|---|
| `priority` | `priority` | `Option<u8>` | 0–9 per iCalendar |
| `free_busy_status` | `freeBusyStatus` | `Option<String>` | per-user |
| `privacy` | `privacy` | `Option<String>` |
| `reply_to` | `replyTo` | `Option<HashMap<String, String>>` |
| `sent_by` | `sentBy` | `Option<String>` |
| `participants` | `participants` | `Option<HashMap<Id, serde_json::Value>>` | assignees extension; Participant objects |
| `request_status` | `requestStatus` | `Option<String>` |

**JSCalendar Alert Properties** (RFC 8984 §4.5, gated on alerts extension):

| Rust field | Wire name | Type |
|---|---|---|
| `use_default_alerts` | `useDefaultAlerts` | `Option<bool>` | per-user |
| `alerts` | `alerts` | `Option<HashMap<Id, serde_json::Value>>` | per-user |

**JSCalendar Multilingual Properties** (RFC 8984 §4.6, gated on multilingual extension):

| Rust field | Wire name | Type |
|---|---|---|
| `localizations` | `localizations` | `Option<HashMap<String, serde_json::Value>>` |

**JSCalendar Time Zone Properties** (RFC 8984 §4.7):

| Rust field | Wire name | Type |
|---|---|---|
| `time_zone` | `timeZone` | `Option<String>` |
| `time_zones` | `timeZones` | `Option<HashMap<String, serde_json::Value>>` | customtimezones extension |

**JSCalendar Task-specific Properties** (RFC 8984 §5.2):

| Rust field | Wire name | Type |
|---|---|---|
| `due` | `due` | `Option<String>` | LocalDateTime |
| `start` | `start` | `Option<String>` | LocalDateTime |
| `estimated_duration` | `estimatedDuration` | `Option<String>` | ISO 8601 Duration |
| `percent_complete` | `percentComplete` | `Option<u8>` | 0–100 |
| `progress` | `progress` | `Option<TaskProgress>` | enum |
| `progress_updated` | `progressUpdated` | `Option<String>` | UTCDateTime |

`TaskProgress` enum values (RFC 8984 §5.2.5): `NeedsAction`, `InProcess`,
`Completed`, `Failed`, `Cancelled`. Wire values: `"needs-action"`,
`"in-process"`, `"completed"`, `"failed"`, `"cancelled"`. An `Other(String)`
catch-all handles vendor-specific and future IANA-registered values.

### Auxiliary types used by Task

**Checklist** (draft-tasks-06 §4.2.3):

| Rust field | Wire name | Type |
|---|---|---|
| `at_type` | `@type` | `String` | always "Checklist" |
| `title` | `title` | `Option<String>` |
| `check_items` | `checkItems` | `Option<Vec<CheckItem>>` |

**CheckItem** (draft-tasks-06 §4.2.3):

| Rust field | Wire name | Type |
|---|---|---|
| `at_type` | `@type` | `String` | always "CheckItem" |
| `title` | `title` | `String` |
| `sort_order` | `sortOrder` | `u32` | default 0 |
| `updated` | `updated` | `Option<String>` | UTCDateTime |
| `is_complete` | `isComplete` | `bool` |
| `assignee` | `assignee` | `Option<Person>` |

**Comment** (draft-tasks-06 §4.2.4):

| Rust field | Wire name | Type |
|---|---|---|
| `at_type` | `@type` | `String` | always "Comment" |
| `message` | `message` | `String` |
| `created` | `created` | `Option<String>` | UTCDateTime |
| `updated` | `updated` | `Option<String>` | UTCDateTime |
| `author` | `author` | `Option<Person>` |

**Person** (draft-tasks-06 §4.2.3, also used in TaskNotification):

| Rust field | Wire name | Type | Notes |
|---|---|---|---|
| `at_type` | `@type` | `String` | always "Person" |
| `name` | `name` | `Option<String>` | |
| `uri` | `uri` | `Option<String>` | scheduleId URI |
| `principal_id` | `principalId` | `Option<String>` | either uri or principalId MUST be set |

### TaskNotification (draft-tasks-06 §5)

| Rust field | Wire name | Type | Notes |
|---|---|---|---|
| `id` | `id` | `Id` | server-set |
| `created` | `created` | `String` | UTCDate; when the notification was created |
| `changed_by` | `changedBy` | `Person` | who made the change |
| `comment` | `comment` | `Option<String>` | iTIP COMMENT, if any |
| `notification_type` | `type` | `NotificationType` | "created" \| "updated" \| "destroyed" |
| `task_id` | `taskId` | `Id` | id of the Task this notification is about |
| `is_draft` | `isDraft` | `Option<bool>` | only for created/updated |
| `task` | `task` | `Option<serde_json::Value>` | Task data before change (updated/destroyed) or after (created) |
| `task_patch` | `taskPatch` | `Option<serde_json::Value>` | PatchObject; updated only |

`NotificationType` enum: `Created`, `Updated`, `Destroyed`. Wire: `"created"`,
`"updated"`, `"destroyed"`.

Note: the spec uses `TaskId` (capital T) as the wire field name in §5.1, but
this appears to be a typo (it should be `taskId` lowercase to be consistent with
camelCase conventions). Document this discrepancy in a code comment; follow the
lowercase camelCase convention.

### TaskNotificationFilterCondition (draft-tasks-06 §5.5.1)

| Rust field | Wire name | Type |
|---|---|---|
| `after` | `after` | `Option<String>` | UTCDate |
| `before` | `before` | `Option<String>` | UTCDate |
| `notification_type` | `type` | `Option<String>` |
| `task_ids` | `taskIds` | `Option<Vec<Id>>` |

### Capability types (draft-tasks-06 §1.6)

`TasksCapability` (session-level, value is an empty object `{}`):
Used as the value at `capabilities["urn:ietf:params:jmap:tasks"]` in the
JMAP session response. No fields.

`TasksAccountCapability` (account-level):

| Rust field | Wire name | Type |
|---|---|---|
| `min_date_time` | `minDateTime` | `String` | LocalDate |
| `max_date_time` | `maxDateTime` | `String` | LocalDate |
| `may_create_task_list` | `mayCreateTaskList` | `bool` |

Sub-capabilities (§§1.6.2–1.6.6) each have their own account-level capability
struct. For `recurrences`: `maxExpandedQueryDuration: Duration`. For `assignees`:
`maxParticipantsPerTask: Option<u64>`. All others are empty objects.

## Key Design Decisions

### Task is a full JSCalendar Task — not a simplified to-do

The spec is explicit: "A Task is a representation of a single task or recurring
series of Tasks in JSTask [RFC8984] format." This means full recurrence support
(via the `recurrences` extension), full participant/assignee support (via the
`assignees` extension), and full alert support (via the `alerts` extension).

Consequence: the `Task` struct has many more fields than a naive to-do type.
However, all fields are `Option`, so simple implementations can ignore the
advanced fields.

### Extension-gated fields still live in the base struct

The six extension capabilities gate behavior, not wire format. A server that
does not advertise `urn:ietf:params:jmap:tasks:recurrences` must still parse
and round-trip a Task that happens to have recurrenceRules. Per the spec (§1.5):
"a Task object might just contain data that the server does not understand. In
this case, the server SHOULD save it and ignore its existence."

We do not use Rust feature flags to gate extension fields. All fields live in
the one `Task` struct; the handler layer checks capability advertisement.

### JSCalendar complex sub-objects are `serde_json::Value`

Location, VirtualLocation, Link, Alert, Participant, RecurrenceRule, TimeZone:
these are fully specified in RFC 8984 but are deeply nested and not needed for
routing or filtering logic. Representing them as `serde_json::Value` defers the
parsing cost and avoids maintaining a full JSCalendar type library in this crate.
When a future crate provides typed JSCalendar types, `Task` can be migrated to
use them with a semver-breaking change.

### `@type` is preserved but not enforced in Task

The wire `"@type": "Task"` is stored as `Option<String>`. Deserialization does
not reject other values; the handler layer enforces correctness.

### TaskList.workflowStatuses has a well-known default

The default is `["completed", "failed", "in-process", "needs-action",
"cancelled", "pending"]` (draft-tasks-06 §3). This default is documented in
comments on the struct field but is not baked into the Rust default — it is a
server responsibility, not a type responsibility.

### TaskRights.mayWriteAll implies other rights

Per the spec: "If [mayWriteAll] is true, the mayWriteOwn, mayUpdatePrivate and
mayRSVP properties MUST all also be true." This invariant is enforced by the
handler/backend, not the type. The type faithfully serializes whatever the server
provides.

### No `isVisible` field

The thin PLAN.md listed `isVisible` but it does not appear in draft-tasks-06 §3.
The TaskList section lists `isSubscribed` and `sortOrder` as the user-preference
fields. There is no `isVisible` field in the spec. Do not add it.

## Module Layout

```
src/
  lib.rs              pub re-exports of all types
  task_list.rs        TaskList, TaskRights, TaskListRole
  task.rs             Task, TaskProgress, Checklist, CheckItem, Comment, Person
  notification.rs     TaskNotification, NotificationType
  filter.rs           TaskFilterCondition, TaskNotificationFilterCondition,
                      TaskComparator, TaskNotificationComparator
  capability.rs       TasksCapability, TasksAccountCapability,
                      TasksRecurrencesAccountCapability,
                      TasksAssigneesAccountCapability
```

## Test Oracle Strategy

Tests must use independent oracles — never derive expected values from the code
under test.

1. RFC 8984 §6.2 (Simple Task) and §6.5 (Task with a Due Date) are copy-pasteable
   JSON fixtures. Use them verbatim.
2. Hand-written JSON constructed directly from the field descriptions in
   draft-tasks-06 §3, §4, and §5.
3. Roundtrip tests (`serialize → deserialize → serialize`) verify serde
   consistency but are not a substitute for spec-grounded oracle tests.

All tests are `#[test]` (no tokio). Fixtures committed in `tests/fixtures/`.

## Dependencies

```toml
jmap-types = { path = "../crate-jmap-types" }
serde      = { version = "1", features = ["derive"] }
serde_json = "1"
# No tokio, no async, no network deps
```
