# Work Logs Plan

## Goal

Build the `WorkLogs` feature in small, test-first steps, ending with a fully integrated
macOS two-panel UI that shows work log entries for the currently selected task.

## Agreed Decisions

- We keep using a feature-first structure.
- Work logs are modeled as a standalone domain model linked to `Task` by `taskID`.
- The initial `WorkLogEntry` is a plain Swift `struct`.
- We start with an in-memory persistence implementation.
- We keep persistence swappable behind a repository protocol.
- A likely future persistence implementation is `SwiftData`.
- All user-visible strings use `String(localized:)` so translations can be added later without structural changes.
- The UI targets macOS first; `NavigationSplitView` is the right primitive for a two-panel layout on macOS 13+.
- Task selection state lives in `ContentView` as `@State var selectedTaskID: Task.ID?`. It can be promoted to a shared environment object later if more views need it.
- `WorkLogListViewModel` is a separate, focused view model. It mirrors the pattern of `TaskListViewModel`.
- Loading work logs is triggered by SwiftUI's `.task(id:)` modifier — it re-fires and auto-cancels whenever `selectedTaskID` changes.
- `TaskListView` receives a `Binding<Task.ID?>` for selection; the `List` drives it natively.
- The work log row shows a time range (`startedAt – endedAt`), duration, and optional description in a single horizontal row — an HStack with fixed-width columns.
- Internationalization of layout (RTL, dynamic type, date/number formatters) is handled for free by using system components; only string literals need `String(localized:)`.

## TDD Rules

- We follow a strict test-first approach.
- Write failing tests first, then implement with a red-green-refactor cycle.
- Keep tests stable during implementation unless requirements change.
- Prioritize strong test coverage.
- We prefer fast unit tests for domain logic.
- Test persistence behavior via the in-memory implementation.

## Project Structure

```text
Sources/App/
  App.swift                                  ← updated: seeds WorkLogRepository, wires both view models
  ContentView.swift                          ← updated: NavigationSplitView + @State selectedTaskID
  Features/
    Tasks/
      Task.swift
      TaskRepository.swift
      InMemoryTaskRepository.swift
      TaskListViewModel.swift
      TaskListView.swift                     ← updated: selection: Binding<Task.ID?>, localized strings
    WorkLogs/
      WorkLogEntry.swift
      WorkLogRepository.swift
      InMemoryWorkLogRepository.swift
      WorkLogListViewModel.swift
      WorkLogListView.swift
      WorkLogRowView.swift

Tests/AppTests/
  Features/
    Tasks/
      ...
    WorkLogs/
      WorkLogEntryTests.swift
      InMemoryWorkLogRepositoryTests.swift
      WorkLogListViewModelTests.swift
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

### Slice 1 — Domain & persistence (complete)

1. Create `WorkLogs` feature folders in `Sources` and `Tests`.
2. Write tests for the `WorkLogEntry` domain model.
3. Implement the minimal `WorkLogEntry` struct to satisfy tests.
4. Write tests for repository behavior.
5. Implement `InMemoryWorkLogRepository` with `fetchEntries(for:)`.

### Slice 2 — Presentation layer (complete)

6. Write tests for `WorkLogListViewModel`:
   - loads entries for a given task ID via the repository
   - exposes loaded entries on `entries`
   - sets `isLoading` correctly during the fetch
   - sets `errorMessage` when the fetch fails
   - exposes an empty `entries` list when the fetch fails
7. Implement `WorkLogListViewModel`:
   - `@MainActor final class`, `ObservableObject`
   - `@Published private(set) var entries: [WorkLogEntry] = []`
   - `@Published private(set) var isLoading = false`
   - `@Published private(set) var errorMessage: String?`
   - `func loadEntries(for taskID: Task.ID) async`

### Slice 3 — Views (complete)

8. Add `WorkLogRowView`:
   - horizontal row with fixed-width columns: time range, duration, description
   - time range: `startedAt` formatted as short time, `–`, `endedAt` formatted as short time (or `"Running"` when `endedAt` is nil)
   - duration: formatted from `duration` when present (e.g. `1h 30m`), otherwise `–`
   - description: optional secondary text, truncated with ellipsis
   - all user-visible strings use `String(localized:)`
9. Add `WorkLogListView`:
   - `List` over `viewModel.entries` rendering `WorkLogRowView` for each entry
   - `overlay` for `isLoading` (`ProgressView`) and `errorMessage` states, matching style of `TaskListView`
   - empty state overlay when entries list is empty
   - `.task(id: taskID)` triggers `viewModel.loadEntries(for: taskID)` and cancels on change
   - all user-visible strings use `String(localized:)`

### Slice 4 — Integration (complete)

10. Update `TaskListView`:
    - add `selection: Binding<Task.ID?>` parameter
    - pass it to `List(viewModel.tasks, selection: selection)`
    - replace hard-coded string `"Unable to Load Tasks"` with `String(localized:defaultValue:)`
11. Update `ContentView`:
    - add `@State private var selectedTaskID: Task.ID?`
    - replace `TaskListView(viewModel:)` with `NavigationSplitView`:
      - sidebar column: `TaskListView(viewModel:, selection: $selectedTaskID)`
      - detail column: `WorkLogListView` when `selectedTaskID != nil`, otherwise a placeholder
    - accept and hold both view models (injected)
12. Update `App.swift`:
    - seed `InMemoryWorkLogRepository` with sample entries linked to the seeded task IDs
    - create `WorkLogListViewModel(repository:)`
    - pass both view models into `ContentView`

## Checkpoints

### Slice 1 — Domain & persistence
- [x] Create `WorkLogs` feature folder structure.
- [x] Write `WorkLogEntry` domain model tests.
- [x] Implement the minimal `WorkLogEntry` struct.
- [x] Write repository behavior tests.
- [x] Implement `InMemoryWorkLogRepository`.

### Slice 2 — Presentation layer
- [x] Write `WorkLogListViewModel` tests.
- [x] Implement `WorkLogListViewModel`.

### Slice 3 — Views
- [x] Add `WorkLogRowView`.
- [x] Add `WorkLogListView`.

### Slice 4 — Integration
- [x] Update `TaskListView` with selection binding and localized strings.
- [x] Update `ContentView` to `NavigationSplitView` with selection state.
- [x] Update `App.swift` with seeded work log data and wired view models.

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

### Presentation

Test `WorkLogListViewModel` behaviour:
- loads entries for a given task ID
- exposes loaded entries
- handles loading failures with an error message
- clears entries on failure
- sets `isLoading` to `false` after load (success and failure)

## Deferred For Later

- write operations (`add`, `update`, `delete`)
- database-backed persistence
- `SwiftData` integration
- iCloud/CloudKit sync
- promoting `selectedTaskID` to a shared environment object / store
- full `.xcstrings` localization catalogue
