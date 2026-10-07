# Architecture decision records

This directory holds the architecture decisions for Time Tracker. Each record is one Markdown file named `NNNN-kebab-title.md`, numbered in decision order. Records use a lightweight format with five sections: Status, Date, Context, Decision, and Consequences.

## Rules

- A record starts as Proposed. Once a decision is approved, its status changes to Accepted.
- Accepted records are immutable. Fixing a typo is fine; changing what was decided is not. Write a new record instead.
- When every decision in a record changes, write a new record and mark the old one Superseded. The old record keeps its content and gains a Status note pointing at the replacement, for example `Superseded by ADR 0009`.
- When only part of a record changes, mark it `Partially superseded by ADR NNNN; all other decisions remain accepted.` The replacement record must name the clauses it supersedes.
- The table below is the index. When you add a record, add its row.

## Index

| Record | Title | Status |
|---|---|---|
| [0001](0001-record-architecture-decisions.md) | Record architecture decisions | Accepted |
| [0002](0002-task-timestamps-and-ordering.md) | Task timestamps and list ordering | Accepted |
| [0003](0003-canonical-timestamp-precision.md) | Canonical timestamp precision | Accepted |
| [0004](0004-paginated-worklog-history.md) | Paginate worklog history by cursor | Partially superseded by ADR 0006 and ADR 0007; all other decisions remain accepted. |
| [0005](0005-worklog-correction.md) | Correct worklog timestamps without overlap | Partially superseded by ADR 0006 and ADR 0007; all other decisions remain accepted. |
| [0006](0006-local-minute-tui-timestamps.md) | Local-minute TUI timestamps | Accepted |
| [0007](0007-completed-worklog-deletion.md) | Delete completed worklogs | Accepted |
| [0008](0008-move-worklogs-between-tasks.md) | Move worklogs between tasks | Accepted |
| [0009](0009-task-search.md) | Search and rank tasks by recent activity | Accepted |
| [0010](0010-share-tui-application-requests.md) | Share TUI application requests across local and remote modes | Accepted |
| [0011](0011-use-one-tui-event-loop-with-a-fixed-redraw-tick.md) | Use one TUI event loop with a fixed redraw tick | Accepted |
| [0012](0012-share-client-query-and-failure-policies.md) | Share client query and failure policies | Partially superseded by ADR 0013; all other decisions remain accepted. |
| [0013](0013-remove-remote-archive-candidate-fingerprints.md) | Remove remote archive candidate fingerprints | Accepted |
| [0014](0014-scope-mutation-checks-to-task-changes.md) | Scope mutation checks to task changes | Partially superseded by ADR 0015; all other decisions remain accepted. |
| [0015](0015-use-github-hosted-runners-for-ci.md) | Use GitHub-hosted runners for CI | Accepted |
| [0016](0016-configurable-inactive-task-archiving.md) | Configurable inactive task archiving | Accepted |
