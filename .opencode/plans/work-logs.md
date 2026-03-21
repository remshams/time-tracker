# Work Logs Plan

## Goal

Build the first `WorkLogs` slice in small, test-first steps.

## Agreed Decisions

- We keep using a feature-first structure.
- Work logs are modeled as a standalone domain model linked to `Task` by `taskID`.
- The initial `WorkLogEntry` is a plain Swift `struct`.
- We start with an in-memory persistence implementation.
- We keep persistence swappable behind a repository protocol.
- A likely future persistence implementation is `SwiftData`.
- Current scope is read-only persistence (`fetchEntries`) to keep the slice small.
- UI integration is intentionally deferred.

## TDD Rules

- We follow a strict test-first approach.
- We write tests before production code.
- We use a red-green-refactor cycle.
- We do not change tests during implementation unless the requirement changes.
- We aim for high test coverage.
- We prefer fast unit tests for domain logic.
- Persistence-specific behavior is tested through the in-memory implementation.

## Project Structure

Initial structure should stay small and feature-focused.

```text
Sources/App/
  Features/
    WorkLogs/
      WorkLogEntry.swift
      WorkLogRepository.swift
      InMemoryWorkLogRepository.swift

Tests/AppTests/
  Features/
    WorkLogs/
      WorkLogEntryTests.swift
      InMemoryWorkLogRepositoryTests.swift
```

## Initial Scope

The first implementation slice includes only the `WorkLogEntry` model and read-only repository.

Initial `WorkLogEntry` fields:
- `id`
- `taskID`
- `description`
- `startedAt`
- `addedAt`
- `endedAt`
- `updatedAt`
- computed `duration`

Initial expectations:
- `endedAt` may be missing for running entries
- if `endedAt` is present, it must not be earlier than `startedAt`
- `updatedAt` must not be earlier than `addedAt`
- repository only supports fetching entries by task ID for now

## Planned Steps

1. Create `WorkLogs` feature folders in `Sources` and `Tests`.
2. Write tests for the `WorkLogEntry` domain model.
3. Implement the minimal `WorkLogEntry` struct to satisfy tests.
4. Write tests for repository behavior.
5. Implement `InMemoryWorkLogRepository` with `fetchEntries(for:)`.

## Checkpoints

- [x] Create `WorkLogs` feature folder structure.
- [x] Write `WorkLogEntry` domain model tests.
- [x] Implement the minimal `WorkLogEntry` struct.
- [x] Write repository behavior tests.
- [x] Implement `InMemoryWorkLogRepository`.

## Testing Strategy

### Domain

Test the `WorkLogEntry` model first:
- initialization
- optional description handling
- optional `endedAt` handling
- computed `duration`
- validation behavior
- value equality behavior

### Persistence

Test repository behavior through the in-memory implementation:
- returns seeded entries for a known task ID
- returns an empty list when no entries exist
- does not return entries from other task IDs

## Deferred For Later

- write operations (`add`, `update`, `delete`)
- UI for work logs
- database-backed persistence
- `SwiftData` integration
- iCloud/CloudKit sync
