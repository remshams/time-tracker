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
4. Add a minimal `addTask` stub to `TaskRepositoryStub` to satisfy protocol conformance (configurable result added in ViewModel slice).
5. Commit persistence layer changes as one focused commit.

## Checkpoints

- [x] Write `addTask` tests for `InMemoryTaskRepository`.
- [x] Extend `TaskRepository` protocol with `addTask`.
- [x] Convert `InMemoryTaskRepository` to `actor` and implement `addTask`.
- [x] Add minimal `addTask` stub to `TaskRepositoryStub` to satisfy protocol (configurable result deferred to ViewModel slice).
- [x] Write `addTask` tests for `TaskListViewModel`.
- [x] Implement `addTask` on `TaskListViewModel`.
- [x] Add task-creation UI (sheet with title + description fields).

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

---

# Add Task — UI Slice

## Goal

Surface task creation through a sheet triggered by a toolbar `+` button in the sidebar, following the agreed macOS/SwiftUI patterns.

## Agreed Decisions

- A `+` toolbar button is placed in `TaskListView` using `.toolbar { ToolbarItem(placement: .primaryAction) }`. This is the standard macOS pattern (used by Reminders, Notes, Xcode).
- The button is also reachable via `⌘N` keyboard shortcut.
- Tapping the button sets `@State private var isAddingTask = false` to `true`, which drives a `.sheet(isPresented:)`.
- The sheet is implemented as a new `AddTaskView` — a thin form view with no repository knowledge.
- `AddTaskView` holds only local `@State` fields: `title: String` and `description: String`. These are ephemeral — discarded on cancel, passed upward on save.
- `AddTaskView` exposes an `onSave: (String, String) -> Void` closure callback. This is the standard Swift/SwiftUI pattern for directed child-to-parent communication (equivalent to `@Output EventEmitter` in Angular).
- The Save button is disabled when `title` is empty or whitespace-only (client-side guard; repository also validates).
- `TaskListView` is the thin bridge: it owns `isAddingTask`, presents the sheet, and in `onSave` calls `Task { await viewModel.createTask(title:description:) }` then sets `isAddingTask = false`.
- `TaskListViewModel.createTask(title:description:)` is the orchestrator: it calls the repository and then reloads the task list. This keeps the view model as the single owner of repository access — consistent with the existing `loadTasks` pattern.
- No dedicated `AddTaskViewModel` is introduced at this stage (YAGNI). The form state is simple enough to live in `@State`.

## Project Structure Changes

```text
Sources/App/
  Features/
    Tasks/
      AddTaskView.swift             ← new: sheet form view
      TaskListView.swift            ← updated: toolbar button + sheet + ⌘N
      TaskListViewModel.swift       ← updated: createTask(title:description:) method

Tests/AppTests/
  Features/
    Tasks/
      TaskListViewModelTests.swift  ← updated: createTask tests
```

## Planned Steps

1. Write failing tests for `TaskListViewModel.createTask(title:description:)`.
2. Implement `createTask(title:description:)` on `TaskListViewModel`.
3. Add `AddTaskView` with `@State` form fields and `onSave` closure.
4. Update `TaskListView` with toolbar `+` button, `⌘N` shortcut, and sheet presentation.

## Checkpoints

- [x] Write `createTask` tests for `TaskListViewModel`.
- [x] Implement `createTask` on `TaskListViewModel`.
- [x] Add `AddTaskView`.
- [x] Update `TaskListView` with toolbar button, shortcut, and sheet.

## Testing Strategy

### Presentation

Test `TaskListViewModel.createTask`:
- calls `repository.addTask` with a `Task` constructed from the given title and description
- reloads tasks after a successful add (published `tasks` reflects the new task)
- sets an error state when the repository throws
- does not update `tasks` when the repository throws

## Deferred For Later

- Inline error display inside the sheet if `createTask` fails (currently surfaces in the list error state)
- Dedicated `AddTaskViewModel` if form complexity grows (e.g. async validation, multi-step forms)
