# Tasks Plan

## Goal

Build the first `Tasks` feature in small, test-first steps.

## Agreed Decisions

- We use a feature-first structure.
- The first feature is `Tasks`.
- We keep the structure intentionally small in the beginning.
- The `Tasks` feature owns its model, persistence boundary, and UI-related code.
- The initial `Task` domain model is a plain Swift `struct`.
- `title` is required.
- Empty or whitespace-only titles are invalid.
- `description` is optional.
- The first UI shows both title and description in a simple list.
- We start with an in-memory persistence implementation.
- We keep persistence swappable behind a repository protocol.
- A likely future persistence implementation is `SwiftData`.
- We may later add task logs and other relationships.
- Broader architectural decisions can later move into a dedicated architecture document.

## TDD Rules

- We follow a strict test-first approach.
- We write tests before production code.
- We use a red-green-refactor cycle.
- We do not change tests during implementation unless the requirement changes.
- We aim for high test coverage.
- We prefer fast unit tests for domain logic.
- Persistence-specific behavior will be tested separately when introduced.

## Project Structure

Initial structure should stay small and feature-focused.

```text
Sources/App/
  App.swift
  Features/
    Tasks/
      Task.swift
      TaskRepository.swift
      InMemoryTaskRepository.swift
      TaskListViewModel.swift
      TaskListView.swift

Tests/AppTests/
  Features/
    Tasks/
      TaskTests.swift
      InMemoryTaskRepositoryTests.swift
      TaskListViewModelTests.swift
```

## Initial Scope

The first implementation slice is only the domain model for `Task`.

Initial `Task` fields:
- `id`
- `title`
- `description`

Initial expectations:
- `title` is mandatory
- `description` is optional
- no completion state yet
- no task logs yet
- no progress tracking yet

## Planned Steps

1. Create the `Tasks` feature folder structure.
2. Write tests for the `Task` domain model.
3. Implement the minimal `Task` struct to satisfy the tests.
4. Write tests for repository behavior.
5. Implement `InMemoryTaskRepository`.
6. Write tests for `TaskListViewModel`.
7. Implement `TaskListViewModel`.
8. Add a minimal SwiftUI list that shows task title and description.

## Checkpoints

- [x] Create the `Tasks` feature folder structure.
- [x] Write `Task` domain model tests.
- [x] Implement the minimal `Task` struct.
- [x] Write repository behavior tests.
- [x] Implement `InMemoryTaskRepository`.
- [x] Write `TaskListViewModel` tests.
- [x] Implement `TaskListViewModel`.
- [x] Add a minimal task list UI showing title and description.

## Testing Strategy

### Domain

Test the `Task` model first:
- initialization
- required title handling
- optional description handling
- value semantics
- equality behavior if we adopt `Equatable`

### Persistence

Test repository behavior through the in-memory implementation:
- returns seeded tasks
- returns an empty list when no tasks exist

### Presentation

Test the view model behavior:
- loads tasks
- exposes loaded tasks
- handles loading failures correctly

## Deferred For Later

- database-backed persistence
- `SwiftData` integration
- iCloud/CloudKit sync
- task logs
- progress tracking
- completion state
- broader architecture documentation

---

# Add Task — Feature Slice

## Goal

Allow a new task to be created and persisted through the repository layer, following the same test-first approach used in the initial Tasks slice.

## Agreed Decisions

- `TaskRepository` gains a new `addTask(_ task: Task) async throws` method.
- `addTask` is `throws` to match expected future behaviour of `SwiftData`-backed persistence.
- `InMemoryTaskRepository` is converted from a `struct` to an `actor` to safely hold mutable state across concurrent calls.
- The `actor` satisfies `Sendable` automatically — no manual locking required.
- `TaskListViewModel` will gain an `addTask(title:description:)` method in a subsequent slice.
- The UI sheet for task creation comes after the ViewModel slice.

## Planned Steps

1. Write failing tests for `addTask` in `InMemoryTaskRepositoryTests`.
2. Extend `TaskRepository` protocol with `addTask(_ task: Task) async throws`.
3. Convert `InMemoryTaskRepository` from `struct` to `actor` and implement `addTask`.
4. Update `TaskRepositoryStub` in tests: add a configurable `addTaskResult`.
5. Commit persistence layer changes as one focused commit.

## Checkpoints

- [x] Write `addTask` tests for `InMemoryTaskRepository`.
- [x] Extend `TaskRepository` protocol with `addTask`.
- [x] Convert `InMemoryTaskRepository` to `actor` and implement `addTask`.
- [x] Update `TaskRepositoryStub` with configurable `addTask` result.
- [ ] Write `addTask` tests for `TaskListViewModel`.
- [ ] Implement `addTask` on `TaskListViewModel`.
- [ ] Add task-creation UI (sheet with title + description fields).

## Testing Strategy

### Persistence

Test `InMemoryTaskRepository.addTask`:
- an added task appears in a subsequent `fetchTasks()` call
- multiple tasks can be added one after another
- tasks seeded at initialisation are preserved after `addTask`

### Presentation (next slice)

Test `TaskListViewModel.addTask`:
- calls through to `repository.addTask`
- appends the new task to the published `tasks` array on success
- sets an error state when the repository throws

## Deferred For Later

- `SwiftData`-backed `addTask` implementation
- duplicate-title validation (not required yet)
- task editing and deletion
