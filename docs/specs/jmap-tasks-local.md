---
title: "JMAP for Tasks: the crate-jmap working profile"
abbrev: JMAP Tasks (crate-jmap)
docname: crate-jmap-tasks-local-00
category: info
date: 2026-11-20

# Not an Internet-Draft. ipr "none" and the independent stream only make
# kramdown-rfc and xml2rfc render it; set real values (and add the IETF
# boilerplate) only if this text is ever submitted.
ipr: none
submissiontype: independent
stand_alone: yes
smart_quotes: no
pi: [toc, sortrefs, symrefs]

author:
  -
    fullname: Mark Atwood
    organization: Review Commit, LLC
    email: mark@reviewcommit.com

contributor:
  -
    fullname: Joris Baum
    organization: audriga
    contribution: Editor of draft-ietf-jmap-tasks-06, on which this document is based.
  -
    fullname: Hans-Jörg Happel
    organization: audriga
    contribution: Editor of draft-ietf-jmap-tasks-06, on which this document is based.

normative:
  RFC2119:
  RFC3986:
  RFC5546:
  RFC8174:
  RFC8620:
  RFC8984:
  RFC9670:
  JMAP-TASKS-06:
    title: "JMAP for Tasks"
    author:
      - fullname: Joris Baum
      - fullname: Hans-Jörg Happel
    seriesinfo:
      Internet-Draft: draft-ietf-jmap-tasks-06
    date: 2023-03-10
    annotation: "Expired 2023-09-11. Editor's source: github.com/jmapio/jmap spec/tasks/*.mdown at cd67de0."
  JMAP-CALENDARS:
    title: "JSON Meta Application Protocol (JMAP) for Calendars"
    author:
      - fullname: Neil Jenkins
      - fullname: Michael Douglass
    seriesinfo:
      Internet-Draft: draft-ietf-jmap-calendars-34
    date: 2026-11-15
  JMAP-OBJECT-HISTORY:
    title: "JMAP Object History"
    author:
      - fullname: Bron Gondwana
      - fullname: Joel Coffey
    seriesinfo:
      Internet-Draft: draft-ietf-jmap-object-history-03
    date: 2026

informative:
  JSCALENDARBIS:
    title: "JSCalendar: A JSON Representation of Calendar Data"
    author:
      - fullname: Neil Jenkins
      - fullname: Robert Stepanek
    seriesinfo:
      Internet-Draft: draft-ietf-calext-jscalendarbis-22
    date: 2026-10
  JMAPIO-ITEMS:
    title: "JMAP Tasks issues and pull requests #456 to #472"
    target: https://github.com/jmapio/jmap/issues?q=456..472
    author:
      fullname: Mark Atwood
    date: 2026-11
  JMAP-UPDATES:
    title: "JMAP Updates"
    author:
      fullname: Mark Atwood
    seriesinfo:
      Internet-Draft: draft-atwood-jmap-updates-00
    date: 2026

--- abstract

This document specifies the data model and methods that crate-jmap's
Tasks crates (`jmap-tasks-types`, `jmap-tasks-server`,
`jmap-tasks-client`) implement for synchronizing task data with JMAP. It
is draft-ietf-jmap-tasks-06 completed and corrected: the methods that -06
left as "TODO" are defined, the seventeen jmapio/jmap items #456 to #472
are folded in, participant matching, task owners and error names follow
JMAP for Calendars -34, and the IANA section is complete. Every place
where the crates do not do what the text says is marked as an
Implementation note.

--- middle

# Introduction

## Status of this document {#status}

This is crate-jmap's working profile of JMAP for Tasks. It is **not** an
Internet-Draft and not an IETF document, and nothing in it has IETF
consensus. It records what crate-jmap implements and why, so the
implementation has a single written reference while the IETF work on
task lists restarts (docs/design/DECISIONS.md decision 52). It will be
offered to the new IETF task-lists effort once that effort is under way;
until then it is not posted anywhere.

The base text is draft-ietf-jmap-tasks-06 {{JMAP-TASKS-06}} by Joris Baum
and Hans-Jörg Happel (audriga), and its editor's source in the
jmapio/jmap repository (spec/tasks/*.mdown at commit cd67de0). Much of
the text below is theirs, adapted. Text imported to fill the -06 "TODO"
sections comes from JMAP for Calendars -34 {{JMAP-CALENDARS}} by Neil
Jenkins and Michael Douglass. The changes from -06, each with its source,
are listed in {{changes}}.

Sources of the changes:

- the jmapio/jmap items filed by Mark Atwood from implementing -06
  {{JMAPIO-ITEMS}}: issues #456, #457, #459, #461, #463, #465, #466,
  #468, #470, #471 and pull requests #458, #460, #462, #464, #467, #469,
  #472 (audited against the crates in
  docs/audits/2026-11-19-jkl8.1/README.md);
- crate-jmap design decisions 23, 42, 43 and 52
  (docs/design/DECISIONS.md);
- alignment with JMAP for Calendars -34;
- the "Supports History" column of {{JMAP-OBJECT-HISTORY}} Section 7.2.

## Implementation notes {#impl-notes-convention}

Paragraphs headed **Implementation note** describe where crate-jmap (at
the commit that last changed this file) differs from the normative text
around them: a rule the crates do not enforce, enforce differently, or
leave to the backend. They are not part of the specification. {{impl-index}}
lists them all.

crate-jmap is a library kit, not a server: the handlers in
`jmap-tasks-server` parse, validate and route requests, and a
`TasksBackend` that the deploying server writes does storage and
permission checks. "The crates" below means the types and handlers;
"the reference backend" means the demonstration `MemoryBackend`
(feature `memory`), which is single-user and in memory.

## Notational Conventions

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT",
"SHOULD", "SHOULD NOT", "RECOMMENDED", "NOT RECOMMENDED", "MAY", and
"OPTIONAL" in this document are to be interpreted as described in BCP 14
{{RFC2119}} {{RFC8174}} when, and only when, they appear in all
capitals, as shown here.

Type signatures, examples, and property descriptions in this document
follow the conventions established in Section 1.1 of {{RFC8620}}. Data
types defined in the core specification are also used in this document.

## Data types

`LocalDateTime`
: A string in the same format as `UTCDate` ({{RFC8620}} Section 1.4) but
  with the `time-offset` omitted, for example `2014-10-30T14:12:00`, as
  defined in {{RFC8984}} Section 1.4.4. Its absolute time depends on a
  time zone. -06 called this type `LocalDate`; the meaning is the same.

`Duration`
: A length of time in the subset of ISO 8601 defined in {{RFC8984}}
  Section 1.4.6.

`TimeZoneId`
: A time zone id from the IANA Time Zone Database, or a custom time zone
  id where the customtimezones extension allows it ({{RFC8984}}
  Section 4.7).

## Terminology

The terminology of {{RFC8620}} Section 1.6 applies. TaskList, Task and
TaskNotification name the data types defined here and instances of them.
ParticipantIdentity is the data type defined in {{JMAP-CALENDARS}}
Section 3 ({{identity}}).

## Data Model Overview

An Account ({{RFC8620}} Section 1.6.2) contains zero or more TaskList
objects. A TaskList is a named collection of Tasks belonging to a
Principal ({{RFC9670}}). Task lists can also provide defaults, such as
alerts and a color, to apply to tasks in them. Clients commonly let
users toggle the visibility of tasks belonging to a particular task
list.

A Task is a single task or a recurring series of tasks, represented as a
JSCalendar Task object ({{RFC8984}} Section 5.2) with the JMAP
properties of {{task}}. Recurrence rules and alerts work as in
{{JMAP-CALENDARS}}.

As CalendarEventNotification does for events, TaskNotification records
changes made to a task by other users. ShareNotification ({{RFC9670}})
tells the user when their access to another user's task list is granted
or revoked.

Task systems vary. Only the core capability, `urn:ietf:params:jmap:tasks`,
MUST be implemented; support for recurrences, assignees, alerts,
localizations and custom time zones is advertised separately
({{capabilities}}). As {{RFC8620}} defines, servers SHOULD answer a
request that uses a capability they do not understand with an
`urn:ietf:params:jmap:error:unknownCapability` error. A Task object may
still contain data the server does not understand; the server SHOULD
store it and otherwise ignore it.

> **Implementation note:** the handlers do not check that a property of
> an extension (for example `recurrenceRules`, `participants`, `alerts`)
> is sent only when that extension's capability is advertised or in
> `using`. Every Task property is accepted and stored, which is the
> "SHOULD store it" case above applied to all extensions.

## Relationship to JMAP for Calendars {#rel-calendars}

-06 described Tasks by outlining only its differences from JMAP for
Calendars. This document keeps that relationship but states every method
in full, so a reader needs {{JMAP-CALENDARS}} only for the parts it
names: ParticipantIdentity (Section 3), the patching rules of
CalendarEvent/set (Section 5.9.1), recurrence expansion and "this and
future" updates (Sections 5.5 and 5.6), and attachments (Section 5.3).
Section numbers refer to draft-ietf-jmap-calendars-34.

Where -34 depends on {{JSCALENDARBIS}} (notably the Participant
`calendarAddress` property), this document follows -34. Task objects are
otherwise {{RFC8984}} Task objects.

## Addition to the Capabilities Object {#capabilities}

The capabilities object is returned as part of the JMAP Session object;
see {{RFC8620}} Section 2. This document defines six capability URIs.
For each, the value in the Session `capabilities` property is an empty
object.

### urn:ietf:params:jmap:tasks

Support for the core properties of TaskList, Task and TaskNotification
and their methods. The value in an account's `accountCapabilities`
property is an object that MUST contain:

- **minDateTime**: `LocalDateTime`
  The earliest date-time the server is willing to accept for any date
  stored in a Task.
- **maxDateTime**: `LocalDateTime`
  The latest date-time the server is willing to accept for any date
  stored in a Task.
- **mayCreateTaskList**: `Boolean`
  If true, the user may create a task list in this account.

### urn:ietf:params:jmap:tasks:recurrences

Support for the recurrence properties ({{ext-recurrences}}) and
`expandRecurrences` in Task/query. The account capability value MUST
contain:

- **maxExpandedQueryDuration**: `Duration`
  The maximum duration the user may query over when asking the server to
  expand recurrences.

### urn:ietf:params:jmap:tasks:assignees

Support for participants ({{ext-assignees}}). The account capability
value MUST contain:

- **maxParticipantsPerTask**: `UnsignedInt|null`
  The maximum number of participants a single task may have, or null for
  no limit.

### urn:ietf:params:jmap:tasks:alerts

Support for alerts ({{ext-alerts}}). The account capability value is an
empty object.

### urn:ietf:params:jmap:tasks:multilingual

Support for localizations ({{ext-multilingual}}). The account capability
value is an empty object.

### urn:ietf:params:jmap:tasks:customtimezones

Support for custom time zones ({{ext-customtimezones}}). The account
capability value is an empty object.

> **Implementation note:** `jmap-tasks-types` provides the twelve
> capability types (`TasksCapability`, `TasksAccountCapability`, ...),
> but advertising them in the Session is the deploying server's job; the
> handlers never read the account capabilities. In particular
> `minDateTime`, `maxDateTime` and `maxParticipantsPerTask` are not
> enforced by Task/set ({{task-set}}). `maxExpandedQueryDuration` is
> enforced, through `TasksBackend::limits`.

# Principals and Sharing

For systems that also support JMAP Sharing {{RFC9670}}, the presence of
the `urn:ietf:params:jmap:tasks` capability in a Principal's
`capabilities` property indicates that the Principal may be used with
task management.

## Principal Capability urn:ietf:params:jmap:tasks

A `urn:ietf:params:jmap:tasks` property is added to the Principal
`capabilities` object. Its value is an object with these properties:

- **accountId**: `Id|null`
  Id of the Account with the `urn:ietf:params:jmap:tasks` capability
  that contains the task data for this Principal, or null if either
  there is none (for example, the Principal is a group used only for
  permissions management) or the user has no access to any data in that
  account. The Account object is in the Principal's `accounts` property
  ({{RFC9670}} Section 2).
- **mayShareWith**: `Boolean`
  If true, the user may add this Principal as a task list sharee (by
  adding it to the `shareWith` property of a task list, {{tasklist}}).
- **calendarAddress**: `String|null`
  If this Principal may be added as a participant to a task, this is the
  `calendarAddress` to give that participant, used to receive iTIP
  scheduling messages; otherwise null. (-06 had a `sendTo` map here;
  this follows {{JMAP-CALENDARS}} Section 2.1.)

# Participant identities and task owners {#identity}

## Matching a participant to the user

Tasks uses the ParticipantIdentity data type and methods of
{{JMAP-CALENDARS}} Section 3. "The user's ParticipantIdentity objects in
the account" means the ParticipantIdentity objects in that account that
are visible to the authenticated user.

A Participant in a task corresponds to a ParticipantIdentity if the
`calendarAddress` property of the participant is equivalent to the
`calendarAddress` property of the identity after syntax-based
normalization, as per {{RFC3986}} Section 6.2.2. This is the only way a
participant is matched to the user: the -06 rules based on `sendTo` and
`scheduleId` are removed.

A server that advertises `urn:ietf:params:jmap:tasks:assignees` MUST make
the user's ParticipantIdentity objects available in each account that
holds tasks, as {{JMAP-CALENDARS}} Section 3 defines.

> **Implementation note:** the tasks crates do not implement
> ParticipantIdentity (`jmap-calendars-server` does). The owner helpers
> take the user's identity addresses as an argument, and they compare
> `calendarAddress` values as exact strings, without the RFC 3986
> Section 6.2.2 normalization; a backend that stores identities in
> another form normalizes them first (`jmap_tasks_types::is_task_owner`).

## Task owners {#owner}

The user is an **owner** of a task if the Task has a `participants`
property and one of the Participant objects both:

1. has the `owner` role, and
2. corresponds to one of the user's ParticipantIdentity objects in the
   account.

A task has **no owner** if its `participants` property is null or
omitted, or if none of its Participant objects has the `owner` role.

The `chair` role does not make a participant an owner. (-06 defined
owners through a nonexistent `participant` property and the `chair`
role.)

Owners matter for the `mayWriteOwn` right ({{taskrights}}) and for
`hideAttendees` ({{ext-assignees}}).

`jmap-tasks-types` implements this definition as `task_has_owner` and
`is_task_owner`.

# TaskLists {#tasklist}

A TaskList is a named collection of tasks. Every task belongs to exactly
one TaskList.

A **TaskList** object has the following core properties:

- **id**: `Id` (immutable; server-set)
  The id of the task list.
- **role**: `String|null` (default: null)
  Denotes that the task list has a special purpose. If not null, it MUST
  be one of:
  - `inbox`: the principal's default task list;
  - `trash`: this task list holds tasks the user has discarded.
- **name**: `String`
  The user-visible name of the task list. It MUST NOT be the empty string
  and MUST NOT be longer than 255 octets when encoded as UTF-8.
- **description**: `String|null` (default: null)
  An optional longer description of the task list, to give context in
  shared environments.
- **color**: `String|null` (default: null)
  A color for displaying tasks associated with the task list. If not
  null, the value MUST be a case-insensitive color name from Section 4.3
  of CSS Color Module Level 3, or an RGB value in hexadecimal notation as
  defined in Section 4.2.1 of that document. The color SHOULD have
  sufficient contrast to be used as text on a white background.
- **keywordColors**: `String[String]|null` (default: null)
  A map of keywords to the colors used when displaying tasks with those
  keywords. The rules for `color` apply to each value.
- **categoryColors**: `String[String]|null` (default: null)
  A map of categories to the colors used when displaying tasks with
  those categories. The rules for `color` apply to each value.
- **sortOrder**: `UnsignedInt` (default: 0)
  The sort order of task lists in the client's UI, so it is consistent
  between devices. The number MUST be an integer in the range
  0 <= sortOrder < 2^31. A task list with a lower order should be
  displayed before one with a higher order. Task lists with equal order
  SHOULD be sorted in alphabetical order by name, using the locale's
  character order conventions.
- **isSubscribed**: `Boolean`
  Whether the user has indicated they wish to see this task list in
  their client. This SHOULD default to false for task lists in shared
  accounts the user has access to, and to true for any new task list
  created by the user themselves. If false, the task list should only be
  displayed when the user explicitly asks for it or to offer it for
  subscription.
- **timeZone**: `TimeZoneId|null` (default: null)
  The time zone to use for tasks without a time zone when the server
  needs to resolve them into absolute time, for example for alerts. If
  null, the time zone of the account's Principal is used. Clients SHOULD
  use this as the default for new tasks in this task list, if set.
- **workflowStatuses**: `String[]` (default: `["completed", "failed",
  "in-process", "needs-action", "cancelled", "pending"]`)
  The allowed values of the Task `workflowStatus` property for tasks in
  this list. The default values are the {{RFC8984}} Section 5.2.5
  progress values plus `pending`, which means the task has been created
  and accepted but is on hold. How `workflowStatus` relates to
  `progress` is defined in {{workflow}}.

  Naming and workflows differ between systems. Where a mapping is
  uncertain, these mappings of commonly seen values are recommended:
  - `completed`: `done` (simplest case), `closed`, `verified`, ...
  - `in-process`: `in-progress`, `active`, `assigned`, ...
  - `needs-action`: `not-done` (simplest case), `not-started`, `new`, ...
  - `pending`: `waiting`, `deferred`, `on-hold`, `paused`, ...
- **shareWith**: `Id[TaskRights]|null` (default: null)
  A map of Principal id to the rights of each principal this task list
  is shared with, or null if it is not shared. The principal to which
  the task list belongs MUST NOT be in the map. It may be modified only
  by a user with the `mayAdmin` right. The account id for the principals
  is in the `urn:ietf:params:jmap:principals:owner` capability of the
  Account to which the task list belongs.
- **myRights**: `TaskRights` (server-set)
  The access rights the user has for this TaskList.

> **Implementation note:** `TaskList/get` leaves an unset optional
> property out of the object instead of returning `null` or its default:
> `role`, `description`, `color`, `keywordColors`, `categoryColors`,
> `timeZone`, `workflowStatuses`, `shareWith`, `defaultAlertsWithTime`
> and `defaultAlertsWithoutTime` (serde `skip_serializing_if`). An absent
> `workflowStatuses` is treated as the default list by the
> `workflowStatus` rule ({{workflow}}); the default is not stored.

> **Implementation note:** on TaskList create the handler fills
> `isSubscribed` with true when omitted and sets `myRights` to every
> right true, including `mayDelete` whatever the account's `isReadOnly`;
> a backend with real sharing replaces `myRights`. The handlers do not
> check `role` against the two values (other strings are kept, as
> `TaskListRole::Other`), the `name` length, the `sortOrder` range (the
> type is a 32-bit unsigned integer, so values up to 2^32-1 parse) or the
> color syntax. `name` missing on create is `invalidProperties`.

## TaskRights {#taskrights}

A **TaskRights** object has the following properties:

- **mayReadItems**: `Boolean`
  The user may fetch the tasks in this task list.
- **mayWriteAll**: `Boolean`
  The user may create, modify or destroy all tasks in this task list, or
  move tasks to or from it. If this is true, `mayWriteOwn`,
  `mayUpdatePrivate` and `mayRSVP` MUST also be true.
- **mayWriteOwn**: `Boolean`
  The user may create, modify or destroy a task in this task list if
  either they are an owner of the task ({{owner}}) or the task has no
  owner. So the user may also give up ownership, by updating a task so
  they are no longer an owner. A create is judged on the new task; an
  update or destroy on the task as stored.
- **mayUpdatePrivate**: `Boolean`
  The user may modify the per-user properties ({{per-user}}) of all
  tasks in the task list, even if they could not otherwise modify that
  task. These properties MUST be stored per-user, and changes do not
  affect any other user of the task list. The user may also modify these
  properties on a per-occurrence basis for recurring tasks (by updating
  the `recurrenceOverrides` property).
- **mayRSVP**: `Boolean`
  The user may modify the following properties of any Participant object
  that corresponds to one of the user's ParticipantIdentity objects in
  the account ({{identity}}), even if they could not otherwise modify
  that task:
  - `participationStatus`
  - `participationComment`
  - `expectReply`

  If the task has its `mayInviteSelf` property set to true, the user may
  also add a new Participant to the task whose `calendarAddress` is the
  same as the `calendarAddress` of one of the user's ParticipantIdentity
  objects in the account. The `roles` property of that participant MUST
  only contain `attendee`.

  If the task has its `mayInviteOthers` property set to true and an
  existing Participant in the task corresponds to one of the user's
  ParticipantIdentity objects, the user may also add new participants.
  The `roles` property of any new participant MUST only contain
  `attendee`.

  The user may also do all of the above on a per-occurrence basis for
  recurring tasks (by updating the `recurrenceOverrides` property).
- **mayAdmin**: `Boolean`
  The user may modify sharing (the `shareWith` property) for this task
  list.
- **mayDelete**: `Boolean` (server-set)
  The user may delete the task list itself. This MUST be false if the
  account to which this task list belongs has `isReadOnly` set to true.

When a task is fetched or changed, the user's rights on its task list
apply:

- the user may fetch the task if they have `mayReadItems` on its task
  list;
- the user may move a task out of a task list (by changing the task's
  `taskListId`) if they have `mayWriteAll` on that task list, and into a
  task list if they have `mayWriteAll` on that one;
- the user may make other changes to the task if they have the right to
  do so in its task list.

(-06 copied the Calendars wording for an event in several calendars; a
task is in exactly one list.)

> **Implementation note:** the handlers do no permission checking. The
> `TasksBackend` trait documents every rule of this section as the
> backend's contract (`create_object`, `update_object`,
> `destroy_object`, `update_task_per_user`), including `mayWriteOwn` with
> the owner helpers. The reference backend is single-user (caller
> context `()`) and allows every write, so `mayWriteOwn`,
> `mayUpdatePrivate`-only sharees, `mayRSVP`, `mayInviteSelf`,
> `mayInviteOthers` and `shareWith` under `mayAdmin` are untested
> against it.

## Alerts extension {#tasklist-alerts}

With `urn:ietf:params:jmap:tasks:alerts`, a TaskList also has:

- **defaultAlertsWithTime**: `Id[Alert]|null`
  A map of alert ids to Alert objects ({{RFC8984}} Section 4.5.2) to
  apply to tasks where `showWithoutTime` is false and `useDefaultAlerts`
  is true. Ids MUST be unique across all default alerts in the account,
  including those in other task lists; a UUID is recommended. If omitted
  on creation, the default is server dependent: for example, servers may
  always default to null, or copy the alerts of the default task list.
- **defaultAlertsWithoutTime**: `Id[Alert]|null`
  As `defaultAlertsWithTime`, for tasks where `showWithoutTime` is true.

## TaskList/get

This is a standard "/get" method as described in {{RFC8620}}
Section 5.1. The `ids` argument may be null to fetch all at once.

## TaskList/changes

This is a standard "/changes" method as described in {{RFC8620}}
Section 5.2.

## TaskList/set {#tasklist-set}

This is a standard "/set" method as described in {{RFC8620}} Section 5.3,
with the following extra argument:

- **onDestroyRemoveTasks**: `Boolean` (default: false)
  If false, an attempt to destroy a TaskList that still has Tasks in it
  is rejected with a `taskListHasTask` SetError. If true, the Tasks in
  the TaskList are destroyed along with it.

The `shareWith` property may only be set by users with the `mayAdmin`
right. When modifying `shareWith`, the user cannot give a principal a
right that the principal did not already have unless the user making the
change also has that right. Such an attempt is rejected with a
`forbidden` SetError.

Users subscribe to or unsubscribe from a task list by setting
`isSubscribed`. The server MAY forbid users from subscribing to some
task lists even though they may see them, rejecting the update with a
`forbidden` SetError.

The following properties may be set by anyone subscribed to the task
list, and are stored per-user:

- `name`
- `color`
- `sortOrder`
- `timeZone`
- `defaultAlertsWithTime`
- `defaultAlertsWithoutTime`

`name`, `color` and `timeZone` are initially inherited from the owner's
copy of the task list; once a sharee sets one, the sharee has their own
copy, which later changes to the owner's value do not overwrite.
`sortOrder`, `defaultAlertsWithTime` and `defaultAlertsWithoutTime`
initially have their default value for each sharee; they are not
inherited from the owner.

The following extra SetError type is defined:

For "destroy":

- **taskListHasTask**: The TaskList has at least one Task in it, and the
  `onDestroyRemoveTasks` argument was false.

> **Implementation note:** `onDestroyRemoveTasks: true` is done by the
> handler: it finds the list's Tasks with an `inTaskLists` query,
> destroys each through `destroy_object`, and then destroys the list; if
> a Task cannot be destroyed, the list stays and its `notDestroyed` entry
> carries that error. (The `destroy_object` rustdoc in
> `jmap-tasks-server/src/backend.rs` still says the cascade is not done,
> bd:JMAP-ji1v; the handler does it.) The per-user TaskList properties
> are the backend's business; the reference backend has one user.

# Tasks {#task}

A **Task** object is a JSCalendar Task object ({{RFC8984}} Section 5.2)
with the JMAP properties below. As use cases vary, the JSCalendar parts
are split into the core capability and extensions. Only the core MUST be
implemented. For example, the recurrences extension lets a Task
represent a series of recurring tasks.

The core JSCalendar objects are Task, Link, Location, Relation and
VirtualLocation. The core JSCalendar properties are the Metadata
Properties ({{RFC8984}} Section 4.1), the What and Where Properties
(Section 4.2), the Task Properties (Section 5.2), and `priority`
(Section 4.4.1), `privacy` (Section 4.4.3), `replyTo` (Section 4.4.4)
and `timeZone` (Section 4.7.1).

In addition, a Task has the following core properties:

- **id**: `Id` (immutable; server-set)
  The id of the Task. It uniquely identifies a JSCalendar Task with a
  particular `uid` and `recurrenceId` within the account.
- **taskListId**: `Id`
  The id of the TaskList this task belongs to. A task MUST belong to
  exactly one TaskList at all times, until it is destroyed.
- **isDraft**: `Boolean`
  If true, this task is a draft: the server will not send push
  notifications for its alerts. It may only be set to true on creation;
  once false it cannot be set to true again. This property MUST NOT
  appear in `recurrenceOverrides`.
- **utcStart**: `UTCDate|null`
  The task's `start` converted to UTC using its time zone, or null if
  `start` is not set. It is for simple clients that do not or cannot
  implement time zone support. It is calculated by the server at fetch
  time. Time zones are political and change; fetching the same property
  again may give a different result if the server's time zone data has
  changed, and such a change is not an "update" to the task. It is not
  included by default and must be requested explicitly. A floating task
  (one with a null `timeZone`) is interpreted in the time zone given as
  the Task/get `timeZone` argument ({{task-get}}). Expanding recurrence
  rules or overrides from `utcStart` instead of the local start time is
  not accurate (a weekly recurrence may cross a daylight-saving
  boundary), so clients using `utcStart` are RECOMMENDED to ask the
  server to expand recurrences (`expandRecurrences`, {{task-query}}).
  Setting it is described in {{task-set}}.
- **utcDue**: `UTCDate|null`
  The task's `due` converted to UTC using its time zone, or null if
  `due` is not set. Everything said for `utcStart` applies.
- **sortOrder**: `UnsignedInt` (default: 0)
  The sort order of a task in the client's UI, so it is consistent
  between devices. The number MUST be an integer in the range
  0 <= sortOrder < 2^31. A task with a lower order should be displayed
  before one with a higher order. Tasks with equal order SHOULD be sorted
  in alphabetical order by title, using the locale's character order
  conventions.
- **workflowStatus**: `String|null` (default: null)
  The status of the task in its list's workflow. If not null, it MUST be
  one of the `workflowStatuses` of the task's TaskList. Its relation to
  `progress` is defined in {{workflow}}.

> **Implementation note:** `utcStart` and `utcDue` come from
> `TasksBackend::compute_utc_times`. When the task has the local time but
> the backend returns no value (the trait default, and the reference
> backend, which has no time zone database), the key is left out of the
> object rather than set to null, since null would claim that `start` or
> `due` is unset. A task without `start` / `due` gets null as specified.

> **Implementation note:** every Task property is optional in
> `jmap_tasks_types::Task` and left out when unset, so `taskListId`,
> `isDraft` and the other properties are returned only when stored. The
> reference backend does not check that `taskListId` names an existing
> TaskList; the `TasksBackend::create_object` contract says it is
> `invalidProperties`.

## workflowStatus and progress {#workflow}

`workflowStatus` and the JSCalendar `progress` property ({{RFC8984}}
Section 5.2.5) can express the same states. This section is the one
rule for both. It replaces -06's "If set, `progress` MUST null"
(decision 42, #465).

1. **Mirroring.** When `workflowStatus` is one of the {{RFC8984}}
   Section 5.2.5 progress values (`needs-action`, `in-process`,
   `completed`, `failed`, `cancelled`), the server sets `progress` to the
   same value. For any other value (`pending`, or a value the task list
   defines) the server sets `progress` to null. Whenever the server
   changes `progress` this way it also sets `progressUpdated`
   ({{RFC8984}} Section 5.2.6) to the current time, unless the same
   create or update sets `progressUpdated`. The server reports the
   values it set in the `created` or `updated` response ({{RFC8620}}
   Section 5.3).
2. **Conflict.** A create or update that sets `progress` to anything
   other than the mirrored value while `workflowStatus` is not null is
   rejected with an `invalidProperties` SetError naming `progress`.
   Giving the mirrored value (or null for a non-progress status) is not
   an error.
3. **Not in the list.** A `workflowStatus` that is not one of the task
   list's `workflowStatuses` (the default list, if the task list has
   none) is rejected with an `invalidProperties` SetError naming
   `workflowStatus`. This is checked on create, on an update that sets
   `workflowStatus`, and on an update that moves the task to another
   task list.
4. **List changes.** Changing a TaskList's `workflowStatuses` does not
   touch the tasks already in it. An update of a task that neither sets
   `workflowStatus` nor moves the task is not checked against the list,
   even if the task's current value is no longer in it.
5. **Moving.** Moving a task to a task list whose `workflowStatuses`
   lacks the task's `workflowStatus` is rejected with an
   `invalidProperties` SetError naming `workflowStatus`, unless the same
   update sets `workflowStatus` to a value that list has, or to null.

While `workflowStatus` is null, `progress` is an ordinary client-set
property, and setting `workflowStatus` to null does not change it.

The rule is applied to the task as it would be stored, atomically with
the write, so a concurrent change to the list's `workflowStatuses` cannot
let a value through. `jmap-tasks-server` implements it as
`workflow::apply_workflow_status`, which a backend calls inside the
write.

## Extensions to JSCalendar data types

### Relation

The keys of the `relation` property of a Relation object ({{RFC8984}}
Section 1.4.10) are extended with:

- `depends-on`: this task depends on the referenced task in some manner;
  for example, it may be blocked waiting on it.
- `clone`: the referenced task was cloned from this task.
- `duplicate`: the referenced task is a duplicate of this task.
- `cause`: the referenced task was the cause of this task.

## Additional JSCalendar properties

This document defines four new core JSCalendar properties for the Task
object. (-06 said "two" and registered two; #466.)

### estimatedWork {#estimatedwork}

Type: `UnsignedInt|null` (default: null)

The estimated amount of work the task takes to complete, known in agile
development as complexity or story points. The number has no unit; a
larger number means more work.

### impact {#impact}

Type: `String|null` (default: null)

The impact or severity of the task, for example `minor`, `trivial`,
`major` or `block`. It says nothing about prioritization; the priority
of a task is usually based on its impact and urgency.

### checklists {#checklists}

Type: `Id[Checklist]` (optional)

A map of Checklist ids to Checklist objects. A **Checklist** object has
the following properties:

- **@type**: `String`
  The type of this object. This MUST be `Checklist`.
- **title**: `String` (optional)
  The title of the list.
- **checkItems**: `CheckItem[]` (optional)
  The items of the list.

A **CheckItem** object has the following properties:

- **@type**: `String`
  The type of this object. This MUST be `CheckItem`.
- **title**: `String`
  The title of the item.
- **sortOrder**: `UnsignedInt` (default: 0)
  The sort order of the item in the client's UI. The number MUST be an
  integer in the range 0 <= sortOrder < 2^31. An item with a lower order
  should be displayed before one with a higher order. Items with equal
  order SHOULD be sorted in alphabetical order by title, using the
  locale's character order conventions.
- **updated**: `UTCDateTime` (optional)
  When this item was last updated.
- **isComplete**: `Boolean`
  Whether this item is done.
- **assignee**: `Person` (optional)
  The person this item is assigned to. (-06 spelled this `assigneee`.)

A **Person** object has the following properties, of which at least one
of `uri` and `principalId` MUST be set:

- **@type**: `String`
  The type of this object. This MUST be `Person`.
- **name**: `String` (optional)
  The name of the person.
- **uri**: `String` (optional)
  A URI that identifies the person. When the person is a participant of
  the task, this SHOULD be that participant's `calendarAddress`
  ({{identity}}).
- **principalId**: `Id` (optional)
  The id of the Principal corresponding to the person, if any.

> **Implementation note:** a Person `uri` that is not an RFC 3986 URI is
> rejected with `invalidProperties` naming the top-level property
> (`checklists` or `comments`) on Task/set create and update and on
> Task/copy. This follows the URI rule of {{JMAP-UPDATES}} Section 7.1,
> stricter than this section. The "at least one of `uri` and
> `principalId`" rule is not checked, and `principalId` is typed as a
> plain string in `jmap_tasks_types::Person`. A missing `@type` on
> Checklist, CheckItem, Comment or Person is accepted on input and
> filled in.

### comments {#comments}

Type: `Id[Comment]` (optional)

Free-text comments on this task. A **Comment** object has the following
properties:

- **@type**: `String`
  The type of this object. This MUST be `Comment`.
- **message**: `String`
  The text of the comment.
- **created**: `UTCDateTime` (optional)
  When the comment was created.
- **updated**: `UTCDateTime` (optional)
  When the comment was last updated.
- **author**: `Person` (optional)
  The author of the comment ({{checklists}}).

## Attachments

Attachments are represented as in {{JMAP-CALENDARS}} Section 5.3: a
Link with `rel` `enclosure`, which may carry a `blobId` instead of an
`href`. This is why Task is registered with "Can Reference Blobs: yes"
({{iana-datatypes}}).

> **Implementation note:** the tasks crates do not track blob references
> from Task links and do not translate `blobId` to `data:` URLs; both are
> the deploying server's job.

## Recurrences extension {#ext-recurrences}

With `urn:ietf:params:jmap:tasks:recurrences`, the JSCalendar objects
NDay and RecurrenceRule and the Recurrence Properties ({{RFC8984}}
Section 4.3) are supported, and a Task has the further property:

- **baseTaskId**: `Id|null` (immutable; server-set)
  Defined only when `id` is a synthetic id, generated by the server to
  represent one instance of a recurring Task ({{task-query}}). It is the
  id of the stored Task the instance was generated from.

Recurring tasks, and updates to them, including "this and future"
updates, work as described for events in {{JMAP-CALENDARS}} Sections 5.5
and 5.6.

## Assignees extension {#ext-assignees}

With `urn:ietf:params:jmap:tasks:assignees`, the JSCalendar Participant
object and the Sharing and Scheduling Properties ({{RFC8984}}
Section 4.4) are supported, and a Task has the further property:

- **isOrigin**: `Boolean` (server-set)
  Whether this is the authoritative source for this task: it controls
  scheduling for the task, which was not added as the result of an
  invitation from another task system. It is true if, and only if, the
  task's `replyTo` property is null, or the account will receive
  messages sent to at least one of the methods in `replyTo`.

Task objects MUST NOT have a `method` property; it is used only when
representing iTIP {{RFC5546}} scheduling messages, not tasks in a store.

### Participants

The Participant object ({{RFC8984}} Section 4.4.6) represents
participants. A participant is matched to the user as in {{identity}}.
This document adds one value to the keys of the `roles` property:

- `assignee`: the participant is expected to work on the task.

### mayInviteSelf

Type: `Boolean` (default: false)

If true, any user with access to the task may add themselves to it as a
participant with the `attendee` role, subject to the `mayRSVP` right
({{taskrights}}). The task will also accept "party crasher" replies via
iTIP, subject to other domain-specific restrictions. This property MUST
NOT be set in `recurrenceOverrides`; it may only be set on the base task.

### mayInviteOthers

Type: `Boolean` (default: false)

If true, any current participant with the `attendee` role may add new
participants with the `attendee` role. This property MUST NOT be set in
`recurrenceOverrides`.

### hideAttendees

Type: `Boolean` (default: false)

If true, only the owners of the task ({{owner}}) may see the full set of
participants; other sharees see only the owners and themselves. This
property MUST NOT be set in `recurrenceOverrides`. The filtering of
{{JMAP-CALENDARS}} Section 5.7 applies, with "event" read as "task".

### Per-user properties {#per-user}

In shared task lists, any top-level property registered in the
"JSCalendar Properties" registry with "Is Per-User: Yes" MUST be stored
per-user. This includes:

- `keywords`
- `color`
- `freeBusyStatus`
- `useDefaultAlerts`
- `alerts`

These are the properties that the `mayUpdatePrivate` right covers. The
user may also modify them on a per-occurrence basis for recurring tasks;
again these MUST be stored per-user. Sharees initially receive the
default value of each, not whatever value another user has set.

When only per-user properties are written, the `updated` property MUST
also be stored just for that user, if set. When `updated` is fetched,
the value returned is the later of the per-user updated time and the
updated time of the base task.

> **Implementation note:** Task/set routes an update whose every patch
> key's first path segment is one of these five properties to
> `TasksBackend::update_task_per_user` (contract: `mayUpdatePrivate`); a
> patch that also touches any other property goes to `update_object` and
> needs write access. The reference backend stores per-user patches on
> the shared task (one user) and never writes `updated`.

## Alerts extension {#ext-alerts}

With `urn:ietf:params:jmap:tasks:alerts`, the JSCalendar objects Alert,
AbsoluteTrigger and OffsetTrigger and the Alerts Properties
({{RFC8984}} Section 4.5) are supported. Default alerts come from the
task list ({{tasklist-alerts}}).

## Multilingual extension {#ext-multilingual}

With `urn:ietf:params:jmap:tasks:multilingual`, the Multilingual
Properties ({{RFC8984}} Section 4.6) are supported.

## Custom Time Zones extension {#ext-customtimezones}

With `urn:ietf:params:jmap:tasks:customtimezones`, the JSCalendar
objects TimeZone and TimeZoneRule and the Time Zone Properties
({{RFC8984}} Section 4.7) are supported.

## Task/get {#task-get}

This is a standard "/get" method as described in {{RFC8620}}
Section 5.1, with one extra argument:

- **timeZone**: `TimeZoneId` (default: `"Etc/UTC"`)
  The time zone to use for floating tasks (tasks with a null `timeZone`)
  when calculating their `utcStart` and `utcDue`. It has no effect unless
  one of those properties is requested.

If `properties` is null or omitted, all properties of the stored Task are
returned except the computed `utcStart` and `utcDue`, which are returned
only when explicitly requested. If either is requested, the
`recurrenceOverrides` property MUST NOT also be requested (overrides
cannot be interpreted accurately with only UTC times); otherwise the
server MUST reject the call with an `invalidArguments` error.

A requested id may be a synthetic id of one instance of a recurring task
(from Task/query with `expandRecurrences`). The server then resolves any
overrides and sets the `start` and `recurrenceId` of the returned Task;
`recurrenceRules` and `recurrenceOverrides` are returned as null if
requested.

The `hideAttendees` and `privacy` filtering of {{JMAP-CALENDARS}}
Section 5.7 applies to Task/get and wherever else task data is returned
or matched, with "event" read as "task" and "calendar" as "task list".

A `timeZone` argument that is neither a string nor null is
`invalidArguments`.

> **Implementation note:** Task/get does not reject `utcStart` or
> `utcDue` requested together with `recurrenceOverrides`, does not take
> the CalendarEvent/get arguments `recurrenceOverridesBefore`,
> `recurrenceOverridesAfter` and `reduceParticipants`, and does no
> `hideAttendees` or `privacy` filtering (none is in -06 either). It does
> not check `timeZone` against the IANA Time Zone Database; an unknown
> zone gives no UTC value.

## Task/changes

This is a standard "/changes" method as described in {{RFC8620}}
Section 5.2. Synthetic instance ids never appear in Task/changes; only
ids of stored Tasks do.

## Task/set {#task-set}

This is a standard "/set" method as described in {{RFC8620}}
Section 5.3. The rules of CalendarEvent/set ({{JMAP-CALENDARS}}
Section 5.9) apply to Task, with "event" read as "task", "calendar" as
"task list", `calendarIds` as `taskListId` and `utcEnd`/`duration` as
`utcDue`/`due`, and with these differences and additions:

- **Patching** follows {{JMAP-CALENDARS}} Section 5.9.1.
- **Synthetic ids.** An update of a synthetic instance id updates the
  recurrence override of that instance on the base task; a destroy
  removes just that instance. A client MUST NOT update or destroy both a
  base task and one of its synthetic instances in one request.
- **Permissions.** Servers MUST enforce the user's rights as returned in
  the TaskList `myRights` ({{taskrights}}) and reject changes that are
  not allowed with a `forbidden` SetError. Existence and permission are
  checked before shape: an update of a task the user cannot see is
  `notFound`, not `invalidProperties` (decision 44). A no-op update
  (`{}`, or only identical values) by a user who may only read the task
  is a success with nothing written (decision 45).
- **Defaults on create.** If omitted on create, the server MUST set
  `@type` to `"Task"`, `uid` to a new globally unique identifier, and
  `created` to the current date-time.
- **updated and sequence.** If the server is the origin of the task
  (`isOrigin` true), it MUST set `updated` to the current time on every
  create and update, overriding a client value, and MUST increment
  `sequence` as {{JMAP-CALENDARS}} Section 5.9 says. Changes only to
  `taskListId`, `isDraft`, `updated` or per-user properties do not
  increment `sequence`.
- **isDraft.** An update whose result has `isDraft: true` for a task
  whose stored value is not true is rejected with `invalidProperties`
  naming `isDraft`.
- **workflowStatus.** {{workflow}} applies to every create, update and
  copy.
- **utcStart and utcDue.** If `utcStart` is set, it is translated into a
  `start` property in the task's time zone using the server's current
  time zone data, and the local time is stored. It MUST NOT be set
  together with `start` and cannot be set inside `recurrenceOverrides`;
  either is rejected with `invalidProperties`. `utcDue` is translated
  into `due` in the same way, and MUST NOT be set together with `due`.
- **method.** The `method` property MUST NOT be set; an attempt is
  rejected with `invalidProperties`.
- **Limits.** A task with more participants than
  `maxParticipantsPerTask`, or with any date earlier than `minDateTime`
  or later than `maxDateTime`, is rejected with `invalidProperties`.
- **taskListId.** A `taskListId` that does not name a TaskList the user
  may write to is rejected with `invalidProperties` naming `taskListId`.
  It may be a creation id reference (`#` followed by the creation id) to
  a TaskList created earlier in the same request.
- **Scheduling.** The `sendSchedulingMessages` argument (Boolean, default
  false) and the `noSupportedScheduleMethods` SetError are as in
  {{JMAP-CALENDARS}} Section 5.9.

> **Implementation note:** the handlers enforce only part of this list.
> They do: server-set `id`, `baseTaskId` and `isOrigin` (a changed value
> is `invalidProperties`, an identical one is dropped); creation id
> references in `taskListId` (an unknown one is `invalidProperties`);
> the per-user routing; type checking of every property against
> `jmap_tasks_types::Task` (`invalidProperties` naming the property). The
> backend contract requires the `isDraft` rule and {{workflow}}, which
> the reference backend implements. Not done anywhere in the crates:
> setting `uid`, `created`, `updated` or `sequence`; computing
> `isOrigin`; rejecting `method`; the `minDateTime`, `maxDateTime` and
> `maxParticipantsPerTask` limits; translating `utcStart`/`utcDue` into
> `start`/`due` (both are accepted as client-settable and the reference
> backend stores them as given); scheduling (`sendSchedulingMessages` is
> not read, and `noSupportedScheduleMethods` is never returned); and
> `privacy` rules.

## Task/copy

This is a standard "/copy" method as described in {{RFC8620}}
Section 5.4. The `privacy` rules of CalendarEvent/copy
({{JMAP-CALENDARS}} Section 5.10) apply. The copy is a create in the
destination account, so {{workflow}} and the other Task/set create rules
apply to it.

## Task/query {#task-query}

This is a standard "/query" method as described in {{RFC8620}}
Section 5.5, with two extra arguments:

- **expandRecurrences**: `Boolean` (default: false)
  If true, the server expands every recurring task: a separate id is
  returned for each instance that matches the query. If true, the filter
  MUST be a single FilterCondition (not a FilterOperator) that has both
  `after` and `before`; otherwise the server MUST reject the call with an
  `invalidArguments` error. This keeps the result finite. If false, a
  single id is returned for a recurring task that matches.
- **timeZone**: `TimeZoneId` (default: `"Etc/UTC"`)
  The time zone in which to read the `after` and `before` conditions,
  and in which to read the local times of floating tasks (tasks with a
  null `timeZone`) when filtering and sorting.

A synthetic instance id is opaque to the client but identifies the base
task and the recurrence id within the account, so the server can resolve
it in Task/get and Task/set. There is no necessary relation between the
ids of instances of the same task.

The following additional errors may be returned instead of the
Task/query response (they are the CalendarEvent/query errors,
{{JMAP-CALENDARS}} Section 5.11):

- **expandDurationTooLarge**: `expandRecurrences` is true and the
  duration between `after` and `before` exceeds the
  `maxExpandedQueryDuration` limit in the account capabilities
  (decision 43; #456 had suggested `tooLarge`).
- **cannotCalculateOccurrences**: the server cannot expand a recurrence
  needed to return the results of this query.

A `timeZone` argument that is neither a string nor null is
`invalidArguments`.

> **Implementation note:** the handler checks the `expandRecurrences`
> filter shape and pre-checks the window against
> `TasksBackend::limits`, reading both `LocalDateTime` bounds as if they
> were UTC; a daylight-saving change inside the window can make the real
> span differ by the offset change. The backend's own check
> (`query_tasks` contract) is the exact one. The reference backend has
> no recurrence engine: with `expandRecurrences` a non-recurring task
> keeps its id and any recurring match is `cannotCalculateOccurrences`.

### Filtering {#task-filter}

A **FilterCondition** object has the following properties, any of which
may be omitted (#456, decision 43):

- **inTaskLists**: `Id[]`
  The task's `taskListId` must be one of these ids.
- **after**: `LocalDateTime`
  The end of the task, or of any recurrence of it, read in the time zone
  given as the `timeZone` argument, must be after this date. The end of
  a task is its `due`, or its `start` if it has no `due`. A task with
  neither does not match. For a floating task, the end is its local
  time read in the `timeZone` argument.
- **before**: `LocalDateTime`
  The beginning of the task, or of any recurrence of it, read in the
  `timeZone` argument, must be before this date. The beginning of a task
  is its `start`, or its `due` if it has no `start`. A task with neither
  does not match.
- **text**: `String`
  Looks for the text in `title`, `description`, `locations` (matching
  the location name), `participants` (matching name or address) and any
  other textual property of the task or any recurrence of it.
- **title**: `String`
  Looks for the text in the task's `title`, or the overridden `title` of
  a recurrence.
- **description**: `String`
  Looks for the text in the task's `description`, or the overridden
  `description` of a recurrence.
- **uid**: `String`
  The task's `uid` is exactly this string.
- **hasKeyword**: `String`
  The task's `keywords` maps this keyword to true.
- **notKeyword**: `String`
  The task's `keywords` does not map this keyword to true.
- **isDraft**: `Boolean`
  The task's `isDraft` is this value.
- **progress**: `String`
  The task's `progress` is exactly this value.
- **workflowStatus**: `String`
  The task's `workflowStatus` is exactly this string.

If `expandRecurrences` is true, all conditions must match the same
instance of a recurring task for that instance to match. Otherwise all
conditions must match, but each may match a different instance.

A FilterCondition with no properties always matches. With several
properties, all must match (as if each were a separate condition under an
AND operator). Text matching follows {{JMAP-CALENDARS}} Section 5.11.1:
case-insensitive, quoted phrases, whitespace-separated tokens that must
all be present, optional stemming.

A FilterCondition property the server does not know is rejected with
`unsupportedFilter`; it MUST NOT be ignored, since a condition with
every property dropped matches every task.

> **Implementation note:** `jmap_tasks_types::TaskFilterCondition` has
> exactly these properties plus the six draft-ietf-jmap-metadata-05
> Section 3.6 conditions, and refuses unknown ones (`unsupportedFilter`).
> Matching is the backend's job. The reference backend evaluates only
> `inTaskLists`, `workflowStatus`, `progress`, `after`, `before` (and
> shared-metadata conditions) in a single FilterCondition; any other
> property, any FilterOperator, and `after`/`before` on a task whose
> `timeZone` is not the query's zone are `unsupportedFilter`.

### Sorting {#task-sort}

The following properties MUST be supported for sorting: `id`, `uid`,
`start`. The following SHOULD be supported: `created`, `updated`.
Sorting by `start` orders tasks by the absolute time they start, not by
the lexical value of `start`; a floating task's start is read in the
`timeZone` argument.

> **Implementation note:** the handler accepts exactly these five sort
> properties and answers any other with `unsupportedSort`, including
> `recurrenceId`, which draft-ietf-jmap-calendars-26 (and so -06) listed
> and -34 dropped. The reference backend sorts by all five, with no
> collation or `i;octet`, `i;ascii-casemap` or `i;unicode-casemap`, and
> breaks ties by id; a task without the sort property comes after the
> tasks with it in ascending order. Having no time-zone database, it
> compares `start` values as strings and answers `unsupportedSort` when
> the matched tasks with a `start` are in more than one time zone (a
> floating task counts as the `timeZone` argument's zone).

## Task/queryChanges

This is a standard "/queryChanges" method as described in {{RFC8620}}
Section 5.6. The `timeZone` argument of Task/query applies to it as
well. A query with `expandRecurrences` true MAY return
`cannotCalculateChanges`.

> **Implementation note:** Task/queryChanges goes through the generic
> `jmap_server::handle_query_changes`, which does not pass `timeZone` or
> `expandRecurrences` to the backend.

# TaskNotifications {#tasknotification}

The TaskNotification data type records changes to tasks in task lists
the user is subscribed to, where the change was not made by the user
themselves. TaskNotifications are created only by the server and are
stored in the same Account as the changed Task. Clients may show them
and let the user dismiss them; to dismiss one, destroy it with
TaskNotification/set. The server rules of CalendarEventNotification
({{JMAP-CALENDARS}} Section 7: when to create them, `privacy` and
`hideAttendees` filtering, a limit on how many are kept, coalescing)
apply, with "event" read as "task".

## Object Properties

A **TaskNotification** object has the following properties:

- **id**: `Id` (immutable; server-set)
  The id of the TaskNotification.
- **created**: `UTCDate`
  The time this notification was created.
- **changedBy**: `Person`
  Who made the change ({{checklists}}).
- **comment**: `String|null`
  The comment sent along with the change by the user who made it (for
  example the COMMENT property in an iTIP message), if any.
- **type**: `String`
  One of `created`, `updated` or `destroyed`.
- **taskId**: `Id`
  The id of the Task this notification is about. (-06 had `TaskId:
  String`.)
- **isDraft**: `Boolean` (created and updated only)
  Whether the task is a draft.
- **task**: `Task`
  The task before the change (updated or destroyed), or after creation
  (created).
- **taskPatch**: `PatchObject` (updated only)
  A patch from the data in `task` to the data after the update.

If the change affects only one instance of a recurring task, the server
MAY set `task` and `taskPatch` for just that instance; `taskId` MUST
still be the id of the stored Task, never a synthetic id.

> **Implementation note:** the crates never create TaskNotifications;
> that is the backend's job, and the reference backend creates none
> (they can be seeded for tests).

## TaskNotification/get

This is a standard "/get" method as described in {{RFC8620}}
Section 5.1.

## TaskNotification/changes

This is a standard "/changes" method as described in {{RFC8620}}
Section 5.2.

## TaskNotification/set

This is a standard "/set" method as described in {{RFC8620}}
Section 5.3. Only destroy is supported; any create or update MUST be
rejected with a `forbidden` SetError.

## TaskNotification/query

This is a standard "/query" method as described in {{RFC8620}}
Section 5.5.

### Filtering

A **FilterCondition** object has the following properties:

- **after**: `UTCDate|null`
  The `created` date must be on or after this date.
- **before**: `UTCDate|null`
  The `created` date must be before this date.
- **type**: `String`
  The `type` property must be this value.
- **taskIds**: `Id[]|null`
  The `taskId` of the notification must be in this list.

### Sorting

The `created` property MUST be supported for sorting.

> **Implementation note:** the reference backend evaluates no
> TaskNotification filter condition and no sort (`unsupportedFilter`,
> `unsupportedSort`); the types and handler accept exactly the ones
> above.

## TaskNotification/queryChanges

This is a standard "/queryChanges" method as described in {{RFC8620}}
Section 5.6.

# Other specifications the crates apply

These are not part of this profile, but the tasks crates implement them
for TaskList, Task and TaskNotification, and a reader of the code meets
them:

- Object history ({{JMAP-OBJECT-HISTORY}}, with the -04 editor's copy
  changes; docs/design/object-history.md) for TaskList and Task, through
  `register_tasks_handlers_with_history`. TaskNotification has no
  history (decision 22).
- draft-ietf-jmap-conditional-02 for the three `/set` methods, through
  `register_tasks_conditional`.
- draft-ietf-jmap-metadata-05: `metadata` and `privateMetadata` on
  TaskList and Task, the `maySetMetadata` right, and the metadata filter
  conditions.
- draft-atwood-jmap-updates-00 {{JMAP-UPDATES}}: a non-null `language`
  argument on Task/query is `invalidArguments` (Task has no localizable
  properties), and the Person `uri` rule above.

# Examples {#examples}

The examples use the account `a1`. Responses show only the properties
the text discusses.

## Fetching task lists

~~~ json
[["TaskList/get", { "accountId": "a1", "ids": null }, "0"]]
~~~

~~~ json
[["TaskList/get", {
  "accountId": "a1",
  "state": "7",
  "list": [{
    "id": "L1",
    "role": "inbox",
    "name": "Personal",
    "sortOrder": 0,
    "isSubscribed": true,
    "workflowStatuses": ["needs-action", "in-review", "completed"],
    "myRights": {
      "mayReadItems": true, "mayWriteAll": true, "mayWriteOwn": true,
      "mayUpdatePrivate": true, "mayRSVP": true, "mayAdmin": true,
      "mayDelete": true
    }
  }],
  "notFound": []
}, "0"]]
~~~

## Creating a task with a workflow status

`completed` is a progress value, so the server mirrors it into
`progress` and sets `progressUpdated` ({{workflow}}):

~~~ json
[["Task/set", {
  "accountId": "a1",
  "create": {
    "k1": {
      "@type": "Task",
      "taskListId": "L1",
      "title": "Renew passport",
      "due": "2026-12-01T09:00:00",
      "timeZone": "America/Los_Angeles",
      "workflowStatus": "completed"
    }
  }
}, "0"]]
~~~

~~~ json
[["Task/set", {
  "accountId": "a1",
  "oldState": "41",
  "newState": "42",
  "created": {
    "k1": {
      "id": "T1",
      "uid": "6a0c2b5e-8f0b-4c3e-9d51-2f3b1d7a9e10",
      "progress": "completed",
      "progressUpdated": "2026-11-20T18:04:11Z"
    }
  }
}, "0"]]
~~~

`in-review` is a list-defined status, so `progress` becomes null:

~~~ json
[["Task/set", {
  "accountId": "a1",
  "update": { "T1": { "workflowStatus": "in-review" } }
}, "0"]]
~~~

~~~ json
[["Task/set", {
  "accountId": "a1",
  "oldState": "42",
  "newState": "43",
  "updated": { "T1": { "progress": null } }
}, "0"]]
~~~

> **Implementation note:** the handler echoes the whole stored Task in
> `created`, and in `updated` whenever the backend changed anything; for
> this update the reference backend removes `progress`, so the echo has
> no `progress` key rather than `"progress": null`.

A conflicting `progress`, or a status the list does not have, is
rejected:

~~~ json
[["Task/set", {
  "accountId": "a1",
  "update": {
    "T1": { "workflowStatus": "completed", "progress": "in-process" }
  },
  "create": {
    "k2": { "@type": "Task", "taskListId": "L1", "title": "x",
            "workflowStatus": "pending" }
  }
}, "0"]]
~~~

~~~ json
[["Task/set", {
  "accountId": "a1",
  "oldState": "43",
  "newState": "43",
  "notCreated": {
    "k2": { "type": "invalidProperties",
            "properties": ["workflowStatus"] }
  },
  "notUpdated": {
    "T1": { "type": "invalidProperties", "properties": ["progress"] }
  }
}, "0"]]
~~~

## Querying with recurrence expansion

~~~ json
[["Task/query", {
  "accountId": "a1",
  "filter": {
    "inTaskLists": ["L1"],
    "after": "2026-12-01T00:00:00",
    "before": "2027-01-01T00:00:00"
  },
  "sort": [{ "property": "start" }],
  "expandRecurrences": true,
  "timeZone": "America/Los_Angeles"
}, "0"]]
~~~

If the window were longer than `maxExpandedQueryDuration`:

~~~ json
[["error", { "type": "expandDurationTooLarge" }, "0"]]
~~~

## Fetching UTC times

~~~ json
[["Task/get", {
  "accountId": "a1",
  "ids": ["T1", "T2"],
  "properties": ["title", "utcStart", "utcDue"],
  "timeZone": "Europe/Berlin"
}, "0"]]
~~~

~~~ json
[["Task/get", {
  "accountId": "a1",
  "state": "43",
  "list": [
    { "id": "T1", "title": "Renew passport",
      "utcStart": null, "utcDue": "2026-12-01T17:00:00Z" },
    { "id": "T2", "title": "Water plants",
      "utcStart": null, "utcDue": null }
  ],
  "notFound": []
}, "0"]]
~~~

## Destroying a task list that has tasks

~~~ json
[["TaskList/set", { "accountId": "a1", "destroy": ["L1"] }, "0"]]
~~~

~~~ json
[["TaskList/set", {
  "accountId": "a1",
  "oldState": "7",
  "newState": "7",
  "notDestroyed": { "L1": { "type": "taskListHasTask" } }
}, "0"]]
~~~

With `"onDestroyRemoveTasks": true` the same call destroys `L1` and its
tasks.

## A task notification

~~~ json
{
  "id": "N1",
  "created": "2026-11-20T18:30:00Z",
  "changedBy": {
    "@type": "Person",
    "name": "Ada Lovelace",
    "uri": "mailto:ada@example.com",
    "principalId": "P2"
  },
  "comment": null,
  "type": "updated",
  "taskId": "T1",
  "isDraft": false,
  "task": { "@type": "Task", "title": "Renew passport" },
  "taskPatch": { "title": "Renew passport (urgent)" }
}
~~~

# Security Considerations {#security}

All security considerations of JMAP {{RFC8620}}, JSCalendar
{{RFC8984}} and JMAP for Calendars {{JMAP-CALENDARS}} Section 9 apply,
with "event" read as "task". In particular:

- Expanding recurrences is costly; servers limit it with
  `maxExpandedQueryDuration` ({{task-query}}) and may refuse with
  `cannotCalculateOccurrences`.
- A shared task exposes its participants, comments and checklist
  assignees to every sharee who can read it. `privacy` and
  `hideAttendees` limit this ({{task-get}}).
- Matching participants to the user by `calendarAddress` lets anyone who
  can claim an address act as that participant. Servers SHOULD restrict
  the addresses a user may claim in ParticipantIdentity, as
  {{JMAP-CALENDARS}} Section 3.3 says.
- TaskNotifications carry full task data, so they MUST be created only
  for users who may see those details.

# IANA Considerations {#iana}

This section is the complete list #466 asks for. It is written as
registrations would be, for use if this text ever becomes an IETF
document; nothing is requested from IANA now.

## JMAP Capability Registrations

IANA will register six JMAP Capabilities as follows. For each:
Specification document: this document; Intended use: common; Change
Controller: IETF; Security and privacy considerations: this document,
{{security}}.

| Capability Name | Section |
|---|---|
| `urn:ietf:params:jmap:tasks` | {{capabilities}} |
| `urn:ietf:params:jmap:tasks:recurrences` | {{capabilities}} |
| `urn:ietf:params:jmap:tasks:assignees` | {{capabilities}} |
| `urn:ietf:params:jmap:tasks:alerts` | {{capabilities}} |
| `urn:ietf:params:jmap:tasks:multilingual` | {{capabilities}} |
| `urn:ietf:params:jmap:tasks:customtimezones` | {{capabilities}} |

(-06's registration for `tasks:assignees` said it registered
`tasks:recurrences`; #466, PR #467.)

## JMAP Data Type Registrations {#iana-datatypes}

IANA will register three entries in the "JMAP Data Types" registry. The
"Supports History" column is the one added by {{JMAP-OBJECT-HISTORY}}
Section 7.2.

| Type Name | Can Reference Blobs | Can Use for State Change | Supports History | Reference |
|---|---|---|---|---|
| TaskList | No | Yes | Yes | {{tasklist}} |
| Task | Yes | Yes | Yes | {{task}} |
| TaskNotification | No | Yes | No | {{tasknotification}} |

For all three, Capability is `urn:ietf:params:jmap:tasks`.

Task can reference blobs through attachment links with `blobId`. The
"Supports History" values follow CalendarEvent/Calendar (Yes) and
CalendarEventNotification (No) in Table 1 of {{JMAP-OBJECT-HISTORY}}.

> **Implementation note:** these rows are not yet in crate-jmap's
> data-type registry (`jmap_types::DATA_TYPES`, decision 14); Task and
> TaskList get history through
> `jmap_object_history_types::PENDING_HISTORY_REGISTRATION` instead.
> Moving them into the registry and emptying that list is bd
> JMAP-tna1.3.26, which this document unblocks.

## JMAP Error Codes Registry

IANA will register one error code in the "JMAP Error Codes" registry:

- JMAP Error Code: `taskListHasTask`
- Intended use: common
- Change controller: IETF
- Reference: this document, {{tasklist-set}}
- Description: The TaskList has at least one Task in it, and the
  `onDestroyRemoveTasks` argument was false.

Task/query also returns `expandDurationTooLarge` and
`cannotCalculateOccurrences`, which {{JMAP-CALENDARS}} Section 10.8
registers; IANA will add this document, {{task-query}}, as a further
reference to those two entries. (#456 had proposed `tooLarge`;
decision 43.)

## JSCalendar Properties Registry

In the entries below, "Since Version" is 2.0 as in {{JMAP-CALENDARS}}
Section 10.9, Change Controller is IETF, and Is Per-User is No.

### Properties already registered

{{JMAP-CALENDARS}} Section 10.9.1 already registers these with Property
Context "Event, Task". This document uses them for Task with the same
meaning and asks for no change: `id`, `isDraft`, `isOrigin`, `utcStart`,
`mayInviteSelf`, `mayInviteOthers`, `hideAttendees`, and `blobId`
(context Link). Its update of `useDefaultAlerts` (Is Per-User: Yes,
Section 10.9.2) applies to Task too. `calendarIds`, `baseEventId` and
`utcEnd` are reserved for Task by Calendars but are not used by this
document.

(-06 said "Some IANA registrations for JSTask are described in JMAP for
Calendars. TODO explicitly list them."; #466.)

### Added properties

| Property Name | Property Type | Property Context | Intended Usage | Reference |
|---|---|---|---|---|
| taskListId | not applicable | Task | reserved | {{task}} |
| baseTaskId | not applicable | Task | reserved | {{ext-recurrences}} |
| utcDue | not applicable | Task | reserved | {{task}} |
| sortOrder | not applicable | Task | reserved | {{task}} |
| workflowStatus | not applicable | Task | reserved | {{workflow}} |
| estimatedWork | UnsignedInt | Task | common | {{estimatedwork}} |
| impact | String | Task | common | {{impact}} |
| checklists | `Id[Checklist]` | Task | common | {{checklists}} |
| comments | `Id[Comment]` | Task | common | {{comments}} |
| @type | String | Checklist, CheckItem, Comment, Person | common | {{checklists}} |
| title | String | Checklist, CheckItem | common | {{checklists}} |
| checkItems | `CheckItem[]` | Checklist | common | {{checklists}} |
| sortOrder | UnsignedInt | CheckItem | common | {{checklists}} |
| updated | UTCDateTime | CheckItem, Comment | common | {{checklists}} |
| isComplete | Boolean | CheckItem | common | {{checklists}} |
| assignee | Person | CheckItem | common | {{checklists}} |
| message | String | Comment | common | {{comments}} |
| created | UTCDateTime | Comment | common | {{comments}} |
| author | Person | Comment | common | {{comments}} |
| name | String | Person | common | {{checklists}} |
| uri | String | Person | common | {{checklists}} |
| principalId | Id | Person | common | {{checklists}} |

The JMAP-level properties (`taskListId`, `baseTaskId`, `utcDue`,
`sortOrder`, `workflowStatus`) are reserved, as Calendars reserves its
own, so the names cannot be given another meaning on a Task. -06
registered only `estimatedWork` (as `UnignedInt|null`) and `impact`.

## JSCalendar Types Registry

IANA will add these entries to the "JSCalendar Types" registry, each
with Intended Usage common, Since Version 2.0, Change Controller IETF:

| Type Name | Reference |
|---|---|
| Checklist | {{checklists}} |
| CheckItem | {{checklists}} |
| Comment | {{comments}} |
| Person | {{checklists}} |

## JSCalendar Enum Values Registry

IANA will add these values, each with Intended Usage common, Since
Version 2.0, Change Controller IETF:

To "JSCalendar Enum Values for relation (Context: Relation)":

| Enum Value | Reference or Description |
|---|---|
| depends-on | This task depends on the referenced task. |
| clone | The referenced task was cloned from this task. |
| duplicate | The referenced task is a duplicate of this task. |
| cause | The referenced task was the cause of this task. |

To "JSCalendar Enum Values for roles (Context: Participant)":

| Enum Value | Reference or Description |
|---|---|
| assignee | The participant is expected to work on the task. |

> **Implementation note:** nothing to do in the crates: JSCalendar
> relation types and participant roles are open string maps in
> `jmap-jscalendar-types`, so these values are accepted and kept.

--- back

# Implementation notes index {#impl-index}

Every Implementation note above, in document order:

1. Extension properties are accepted whatever is advertised or in
   `using` (Data Model Overview).
2. Capabilities are advertised by the deploying server; `minDateTime`,
   `maxDateTime`, `maxParticipantsPerTask` not enforced (Capabilities).
3. ParticipantIdentity is not in the tasks crates; `calendarAddress`
   compared as exact strings, no RFC 3986 normalization ({{identity}}).
4. TaskList/get omits unset optional properties instead of null or the
   default; `workflowStatuses` default not stored ({{tasklist}}).
5. TaskList create: `isSubscribed` defaults true, `myRights` all true
   (`mayDelete` regardless of `isReadOnly`); `role`, `name` length,
   `sortOrder` range, color syntax unchecked ({{tasklist}}).
6. All TaskRights rules are backend contract; the reference backend is
   single-user and allows everything ({{taskrights}}).
7. `onDestroyRemoveTasks` cascade done by the handler; stale rustdoc on
   `destroy_object`; per-user TaskList properties are backend business
   ({{tasklist-set}}).
8. `utcStart`/`utcDue` key omitted (not null) when the backend cannot
   convert ({{task}}).
9. Task properties all optional and omitted when unset; `taskListId`
   existence unchecked by the reference backend ({{task}}).
10. Person `uri` must be an RFC 3986 URI; the uri-or-principalId rule
    unchecked; `principalId` a plain string; missing `@type` filled
    ({{checklists}}).
11. No blob-reference tracking or `data:` URL translation for
    attachments.
12. Per-user routing only when every patch key is per-user; reference
    backend stores per-user data on the shared task ({{per-user}}).
13. Task/get: no `utcStart`+`recurrenceOverrides` rejection, no
    `recurrenceOverridesBefore`/`After`/`reduceParticipants`, no
    `hideAttendees`/`privacy` filtering, no zone check ({{task-get}}).
14. Task/set: no `uid`/`created`/`updated`/`sequence`, `isOrigin`,
    `method` rejection, date and participant limits, `utcStart`/`utcDue`
    translation, scheduling, or `privacy` rules ({{task-set}}).
15. `expandDurationTooLarge` pre-check reads the bounds as UTC; the
    reference backend cannot expand recurring tasks ({{task-query}}).
16. Filter matching is backend work; the reference backend evaluates
    five conditions, no operators, same-zone `after`/`before` only
    ({{task-filter}}).
17. Sort: the reference backend sorts `start` only when the tasks are
    in one time zone ({{task-sort}}).
18. Task/queryChanges does not pass `timeZone`/`expandRecurrences` to
    the backend.
19. The crates never create TaskNotifications ({{tasknotification}}).
20. The reference backend evaluates no TaskNotification filter or sort.
21. `created`/`updated` echo the whole stored Task; a cleared `progress`
    is an absent key, not null ({{examples}}).
22. The Data Types rows are not yet in `jmap_types::DATA_TYPES`
    (JMAP-tna1.3.26) ({{iana-datatypes}}).
23. Relation and role values need no crate change.

# Open points for the task-lists effort {#open-points}

These are choices this profile makes, or leaves alone, that the new IETF
effort should settle. None changes what crate-jmap implements.

- **Rights name.** Calendars -34 renamed `mayAdmin` to `mayShare`; this
  profile keeps -06's `mayAdmin`.
- **mayRSVP details.** Calendars -34 lets `mayRSVP` change
  `scheduleSequence` and `scheduleUpdated` (not `participationComment`)
  and says a self-added participant's `roles` MUST be omitted, since
  JSCalendarbis obsoletes the `attendee` role. This profile keeps -06's
  list and `attendee`, changing only `sendTo` to `calendarAddress`.
- **minDateTime / maxDateTime type.** Calendars -34 made them
  `UTCDateTime`, reading floating values as UTC; this profile keeps
  -06's `LocalDateTime`.
- **Person.** TaskNotification `changedBy` uses the Tasks Person
  (`name`, `uri`, `principalId`), not the Calendars notification Person
  (`name`, `email`, `principalId`, `calendarAddress`). One Person for
  both would be simpler.
- **Filter conditions.** CalendarEvent/query also has `location`,
  `owner` and `participant`; Task/query here does not.
- **Task/get arguments.** `recurrenceOverridesBefore`,
  `recurrenceOverridesAfter` and `reduceParticipants` from
  CalendarEvent/get are not taken over.
- **TaskList role uniqueness.** Neither -06 nor this profile says an
  account has at most one `inbox` or `trash` list.
- **JSCalendar base.** Task objects are RFC 8984, but `calendarAddress`
  comes from JSCalendarbis via Calendars -34. The effort should pick one
  base.
- **ParticipantIdentity without Calendars.** A Tasks-only server needs
  ParticipantIdentity, which only the calendars capability defines.

# Changes from draft-ietf-jmap-tasks-06 {#changes}

One entry per change, with its source. "#N" is a jmapio/jmap issue or
pull request; "decision N" is docs/design/DECISIONS.md.

1. Restated as crate-jmap's local working profile, not an Internet-Draft;
   original authors credited; IETF boilerplate dropped. Source:
   decisions 23, 52.
2. Task/get, Task/set and Task/query, left as "TODO" in -06, are written
   out in full. Source: #456.
3. Task FilterCondition defined: `inTaskLists`, `after`, `before`,
   `text`, `title`, `progress`, `workflowStatus` (#456), plus
   `description` and `uid` from CalendarEvent/query and `hasKeyword`,
   `notKeyword`, `isDraft` as the crate has them. Source: #456;
   crate-jmap `TaskFilterCondition`.
4. `after` and `before` are `LocalDateTime` read in the Task/query
   `timeZone`, over the task span `start`..`due`, not #456's
   `UTCDateTime`. Source: decision 43.
5. Task/get `timeZone` argument added. Source: #456.
6. Task/query `expandRecurrences` and `timeZone` arguments added, with
   the CalendarEvent/query filter requirement. Source: #456.
7. An over-long expansion window is `expandDurationTooLarge` (and
   `cannotCalculateOccurrences` added), not #456's `tooLarge`. Source:
   decision 43; Calendars -34 Section 5.11.
8. Task/query sorting stated (`id`, `uid`, `start` MUST; `created`,
   `updated` SHOULD; `start` by absolute time). Source: Calendars -34
   Section 5.11.2.
9. TaskNotification `TaskId: String` renamed `taskId: Id`. Source: #457,
   PR #458.
10. CheckItem `assigneee` renamed `assignee`. Source: #457, PR #458.
11. TaskList/set prose `TaskListHasTask` corrected to `taskListHasTask`.
    Source: #459, PR #460.
12. Task owner defined by the `participants` property and the `owner`
    role, with no owner when no participant has that role; `chair`
    dropped. Source: #461, PR #462; Calendars -34 Section 4.
13. `utcStart` and `utcDue` are `UTCDate|null`, `start` / `due` converted
    to UTC, null when unset; `utcDue` no longer computed from a
    nonexistent `duration`. Source: #463, PR #464.
14. `workflowStatus` / `progress` rule: the server mirrors a progress
    value into `progress` (and sets `progressUpdated`), other values give
    null; replaces "If set, `progress` MUST null". Source: #465,
    decision 42.
15. The four error and no-op cases: conflicting `progress` and a status
    not in the list are `invalidProperties`; changing a list's
    `workflowStatuses` does not touch tasks; moving to a list without
    the status is `invalidProperties` unless the update also sets an
    allowed value or null. Source: #465, decision 42.
16. `workflowStatus` null leaves `progress` client-controlled. Source:
    crate-jmap `workflow::apply_workflow_status` (decision 42 is silent).
17. "Defines two new core JSCalendar properties" corrected to four.
    Source: #466.
18. `tasks:assignees` registration no longer says it registers
    `tasks:recurrences`. Source: #466, PR #467.
19. IANA registrations for `checklists` and `comments`. Source: #466.
20. IANA registrations of the Checklist, CheckItem, Comment and Person
    types and their properties. Source: #466.
21. IANA registration of the Relation values `depends-on`, `clone`,
    `duplicate`, `cause`. Source: #466.
22. IANA registration of the Participant role `assignee`. Source: #466.
23. The Calendars registrations Tasks relies on are listed explicitly,
    replacing "TODO explicitly list them"; `taskListId`, `baseTaskId`,
    `utcDue`, `sortOrder`, `workflowStatus` reserved. Source: #466;
    Calendars -34 Section 10.9.
24. JMAP Data Types registrations: TaskList (Supports History Yes), Task
    (Yes, Can Reference Blobs yes), TaskNotification (No). Source:
    decision 23; object-history-03 Section 7.2; Calendars -34
    Section 10.4-10.6.
25. JMAP Error Codes registration of `taskListHasTask`, and this document
    added as a reference for `expandDurationTooLarge` and
    `cannotCalculateOccurrences`. Source: #466 (completeness); Calendars
    -34 Section 10.8.
26. `mayUpdatePrivate` points at the per-user properties instead of an
    empty "following properties" list. Source: #468, PR #469.
27. `isVisible` dropped from the per-sharee defaults sentence (TaskList
    has no such property). Source: #468, PR #469.
28. A participant is matched to the user by `calendarAddress` with
    RFC 3986 normalization, replacing `sendTo` / `scheduleId`. Source:
    #470; Calendars -34 Section 3.
29. Principal capability `sendTo` replaced by `calendarAddress`. Source:
    #470; Calendars -34 Section 2.1.
30. `mayInviteSelf` matches by `calendarAddress`, not `sendTo`. Source:
    #470.
31. Person `uri` SHOULD be the participant's `calendarAddress`, not its
    `scheduleId`. Source: #470.
32. Servers advertising `tasks:assignees` MUST make ParticipantIdentity
    available. Source: #470 (follows from using Calendars' matching).
33. `UnignedInt` corrected to `UnsignedInt` (text and registration).
    Source: #471, PR #472.
34. "MUST null" wording removed (superseded by entry 14). Source: #471,
    PR #472.
35. Trash role "holds messages" corrected to "holds tasks". Source: #471,
    PR #472.
36. Alerts Properties cited as RFC 8984 Section 4.5, not 4.4. Source:
    #471, PR #472.
37. The Participant object cited as RFC 8984, not
    I-D.ietf-calext-jscalendar. Source: #471, PR #472.
38. Stray "s" at the end of task.mdown removed. Source: #471, PR #472.
39. TaskRights for a single task list: -06's "any task list" / "all
    task lists" wording (copied from Calendars, where an event may be in
    several calendars) restated for a task in one list. Source: editorial
    (TaskList definition: "exactly one TaskList").
40. Task/set create defaults, `updated` / `sequence`, `method`, limits,
    `utcStart`/`utcDue` translation, scheduling and `privacy` rules
    imported from CalendarEvent/set by reference. Source: #456 (the
    -06 TODO); Calendars -34 Section 5.9.
41. TaskNotification server rules (creation, filtering, limits) imported
    from CalendarEventNotification; `taskId` never a synthetic id.
    Source: Calendars -34 Section 7.
42. `name` constraint restated as Calendars words it (not empty, at most
    255 octets). Source: Calendars -34 Section 4.
43. A FilterCondition property the server does not know is
    `unsupportedFilter`, never ignored. Source: crate-jmap (bd
    JMAP-6azm); RFC 8620 Section 5.5.
44. `/set` error precedence (existence and permission before shape) and
    the read-only no-op success stated for Task/set. Source: decisions
    44, 45.
45. Section numbers and "XXX" placeholders filled in; Security
    Considerations given content from Calendars -34 Section 9. Source:
    editorial.
46. Implementation notes, the implementation notes index and the open
    points appendix added. Source: decision 52 (a local spec for the
    implementation).

# Acknowledgments
{:numbered="false"}

This document is built on the work of Joris Baum and Hans-Jörg Happel,
editors of draft-ietf-jmap-tasks-06, and on JMAP for Calendars by Neil
Jenkins and Michael Douglass.
