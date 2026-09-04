# 0004: paginate worklog history by cursor

## Status

Accepted

## Date

2026-09-04

## Context

The application can query all worklogs for one task, but the query returns one unbounded `Vec<Worklog>`. The TUI needs a read-only history screen, and a remote client will eventually request the same data over HTTP. Loading every worklog wastes memory and would make response size grow without limit.

Offset pagination is unstable when another client inserts a newer worklog between requests. An insertion shifts later offsets, which can repeat or skip a row. Worklogs may also share a start timestamp, so the timestamp alone cannot define a page boundary.

## Decision

- Worklog history loads only when a task's history screen opens.
- One application request returns at most 50 worklogs.
- History order is start time descending, then `WorklogId` ascending.
- A continuation cursor contains the start time and identifier of the previous page's last worklog. The next query continues strictly after that pair in history order.
- Storage reads one extra row to determine whether an older page exists, then omits that row from the returned page.
- Active worklogs appear in history alongside completed worklogs.
- The TUI keeps the loaded pages as a read-only snapshot. `o` appends an older page, while `r` discards the snapshot and reloads its newest page.
- Each history query refreshes the application's current tracking state. The TUI adopts that state before rendering, so a running row discovered after another client acted uses the same monotonic clock as the header.
- Worklog selection follows `WorklogId`. Appending a page keeps the selected worklog. Refresh keeps it only when it remains on the new first page, otherwise the newest row becomes selected.
- Timestamps display in the terminal's local timezone with an explicit UTC offset. Each instant uses the offset valid at that instant, including across daylight-saving transitions.
- Completed durations come from their stored interval. The active row shares the monotonic elapsed clock used by the timer header.
- The first history screen has no totals, grouping, editing, manual entry, or deletion.

A cursor belongs to one task and one selected store. It is a continuation token, not a durable record identifier. A future HTTP adapter may encode the two cursor fields, but it must preserve the same ordering and page bound.

## Consequences

A task with a long history uses bounded database reads and bounded future HTTP responses. Inserting a newer worklog between page requests does not disturb the older continuation boundary, and the identifier tie-break prevents equal-start rows from repeating or disappearing.

Refreshing deliberately abandons older loaded pages. This avoids combining pages taken from different first-page snapshots. A later worklog edit that changes a start time must use the same reset behavior because the edit changes the ordering key. Refreshing tracking during every page request also keeps a newly discovered active row and the timer header consistent without adding polling or background work.

The SQLite adapter keeps its full-list helper for focused storage probes and tracking tests. User-facing application history goes through the bounded page port.

Exact total duration would require a separate aggregate query. The TUI omits it rather than displaying a subtotal of only the loaded pages.
