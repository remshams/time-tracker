# 0009: search and rank tasks by recent activity

## Status

Accepted

## Date

2026-09-24

## Context

The active and archived task views can be sorted but cannot find a task by name. The worklog move dialog already has fuzzy destination search. Its match score currently controls result order, so a task used long ago may appear above a task edited or tracked today.

## Decision

- Press `/` in either task view to search names with case-insensitive, non-contiguous matching. Search stays within the current active or archived view.
- Rank matches by the newer of `Task.updated_at` and the task's latest worklog start, descending. Tasks without worklogs use `updated_at`. Break ties by creation time descending, then `TaskId` ascending.
- Search ordering applies while a query is open or committed, regardless of the task view's selected normal ordering. Clearing search restores that ordering.
- Enter commits the filter and returns to normal task actions. Escape cancels an edit or clears a committed filter. Switching views clears the filter. Editing and refreshes retain the selected task by identifier while it still matches, then select the first match if it does not.
- The worklog move dialog keeps its fuzzy matching and excludes the source and archived tasks. It uses the same activity, creation, and identifier order as task-list search.

## Consequences

Search can use the task-list read model's latest-work aggregate without loading worklog histories. A rename, archive, restore, or worklog start can change search result order. The move dialog needs each candidate's activity timestamp in addition to its name and identifier. Normal list ordering remains the user's session choice from ADR 0002.
