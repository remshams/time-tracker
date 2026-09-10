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
