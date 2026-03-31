# Work Log Tracking Plan

## Goal

Allow the user to start and stop time tracking for a selected work task directly from
the work log detail view. Clicking **Play** begins a new running `WorkLogEntry`; clicking
**Stop** finalises it by setting `endedAt`, persisting the completed entry, and refreshing
the list. The in-memory repository is extended to support write operations so the feature
can be built and tested end-to-end before a SwiftData persistence layer is introduced.

---

## Agreed Decisions

- A **Play / Stop** toolbar button is placed in `WorkLogListView`, above the work log
  table, using `.toolbar { ToolbarItem(placement: .primaryAction) }` — the same pattern
  used by `WorkTaskListView` for its `+` button.
- The button is disabled while the work log list is loading, to prevent race conditions.
- `WorkLogEntry` gains a computed property `var isRunning: Bool { endedAt == nil }`.
  This is the canonical way to check whether an entry is active. A stored boolean is
  explicitly rejected: `endedAt` already carries the full semantic weight (both *that*
  the entry stopped and *when*), and a separate stored flag would introduce the risk of
  contradictory state with no benefit.
- **`WorkLogTrackingService`** is introduced as a lightweight `@Observable` class that
  holds the currently running entry in memory:
  ```swift
  @Observable
  final class WorkLogTrackingService {
      private(set) var runningEntry: WorkLogEntry?
      var isTracking: Bool { runningEntry != nil }
      func start(_ entry: WorkLogEntry) { runningEntry = entry }
      func stop() { runningEntry = nil }
  }
  ```
  It is created once in `TimeTrackerApp`, bootstrapped at launch by calling
  `fetchRunningEntry()` on the repository, and injected into the SwiftUI environment.
  This is the Swift/SwiftUI equivalent of an Angular singleton service: all reads are
  in-memory (no database round-trip per interaction); the repository is only consulted
  once at startup and on each write.
- **The `WorkTask` is not stored in `WorkLogTrackingService`.** `runningEntry.taskID`
  is always available if a lookup is needed. Storing the task would be redundant,
  keeping an extra reference in sync for no current benefit (YAGNI). If a display
  context ever needs both, a call-site composition (e.g. a `RunningEntryContext` struct)
  is the right approach at that time.
- `WorkLogRepository` gains three new methods:
  - `addEntry(_ entry: WorkLogEntry) async throws` — persists a new entry without any
    business-rule checks. It is a plain data operation.
  - `updateEntry(_ entry: WorkLogEntry) async throws` — persists the stopped (completed)
    entry with `endedAt` and `updatedAt` set.
  - `fetchRunningEntry() async throws -> WorkLogEntry?` — returns the single running
    entry across **all** tasks, or `nil` if none exists. Used **only** at app launch to
    bootstrap `WorkLogTrackingService`. Per-interaction reads go through the service,
    not the repository. A future SwiftData implementation satisfies this with a single
    cheap `FetchDescriptor` predicate on `endedAt == nil`.
- **Cross-task auto-stop is a hard requirement, not an optional enhancement.** Allowing
  two simultaneously running entries is a data integrity violation. The invariant is
  enforced in `WorkLogListViewModel.startTracking`, not in the repository. `startTracking`
  checks `trackingService.runningEntry` first; if one is found it is stopped
  (both `endedAt` and `updatedAt` set to `.now` via `updateEntry` and
  `trackingService.stop()`) before the new entry is created.
- `InMemoryWorkLogRepository` is converted from a `struct` to an `actor` (matching the
  pattern already used by `InMemoryWorkTaskRepository`) to safely support concurrent
  read/write access.
- **`WorkLogListViewModel`** receives `WorkLogTrackingService` as a dependency and gains:
  - `var isTracking: Bool` — delegates to `trackingService.isTracking`.
  - `var trackingError: String?` — set when a write operation fails; cleared on alert
    dismiss. Kept separate from `loadingState` so a write failure never hides the
    already-loaded entry list.
  - `var isShowingTrackingError: Bool` — computed as `trackingError != nil`.
  - `func startTracking(for taskID: WorkTask.ID) async` — checks
    `trackingService.runningEntry`; if present, calls `updateEntry` with `endedAt =
    .now` and `updatedAt = .now` and `trackingService.stop()`; then creates a new
    `WorkLogEntry` with `startedAt = .now`, calls `addEntry`,
    `trackingService.start(newEntry)`, and reloads the list.
  - `func stopTracking() async` — reads `trackingService.runningEntry`, creates an
    updated copy with `endedAt = .now` and `updatedAt = .now`, calls `updateEntry` and
    `trackingService.stop()`, then reloads the list.
- **`updatedAt` on stop**: both `endedAt` and `updatedAt` are always set to `.now`
  together when stopping an entry, satisfying the `updatedAt >= addedAt` validation rule.
- **Write error surfacing** via a SwiftUI `.alert` driven by `isShowingTrackingError`:
  - Chosen over toast/banner: built-in SwiftUI, appropriate for an explicit user action
    that failed, consistent with macOS app conventions.
  - The full-screen `loadingState` overlay is not reused — a write failure must not
    hide an already-loaded entry list.
  - **No Retry button.** The Play/Stop button remains tappable after a write failure
    (only `trackingError` is set, not `loadingState`), so the user retries naturally.
    A Retry action may be added later.
- All user-visible strings use `String(localized:defaultValue:)`.
- In-memory repository retained for this slice; SwiftData deferred.
- Seeded running entry in `TimeTrackerApp` left in place until the add-entry UI is
  available and seed data is no longer needed.

---

## TDD Rules

- Strict test-first: write failing tests first, then implement (red → green → refactor).
- Keep tests stable during implementation unless a requirement changes.
- Prioritise fast unit tests for domain logic and ViewModel behaviour.
- Persistence behaviour tested through the in-memory implementation.

---

## Project Structure Changes

```text
TimeTracker/
  Features/
    WorkLogs/
      WorkLogEntry.swift               ← updated: isRunning computed property
      WorkLogRepository.swift          ← updated: addEntry + updateEntry + fetchRunningEntry
      InMemoryWorkLogRepository.swift  ← updated: actor + mutable store + all three new methods
      WorkLogTrackingService.swift     ← new: @Observable in-memory tracking state
      WorkLogListViewModel.swift       ← updated: trackingService dependency, isTracking,
                                                   trackingError, isShowingTrackingError,
                                                   startTracking, stopTracking
      WorkLogListView.swift            ← updated: Play/Stop toolbar button +
                                                   tracking error alert
  TimeTrackerApp.swift                 ← updated: bootstrap WorkLogTrackingService,
                                                   inject into environment

TimeTrackerTests/
  Features/
    WorkLogs/
      WorkLogEntryTests.swift               ← updated: isRunning tests
      InMemoryWorkLogRepositoryTests.swift  ← updated: addEntry + updateEntry +
                                                        fetchRunningEntry tests
      WorkLogListViewModelTests.swift       ← updated: isTracking, trackingError,
                                                        startTracking, stopTracking tests
```

---

## Planned Steps

### Slice 1 — Domain: `isRunning` computed property

1. Write failing tests for `WorkLogEntry.isRunning` in `WorkLogEntryTests`:
   - `isRunning` is `true` when `endedAt` is `nil`.
   - `isRunning` is `false` when `endedAt` is set.
2. Add `var isRunning: Bool { endedAt == nil }` to `WorkLogEntry`.

### Slice 2 — Repository write operations

3. Write failing tests for `addEntry`, `updateEntry`, and `fetchRunningEntry` in
   `InMemoryWorkLogRepositoryTests`:
   - `addEntry` appends the entry; a subsequent `fetchEntries` returns it.
   - `addEntry` creates the task bucket if none existed for that `taskID`.
   - `addEntry` does not affect entries for other `taskID`s.
   - `updateEntry` replaces the matching entry (by `id`) in the correct task bucket.
   - `updateEntry` throws `.entryNotFound` when the ID does not exist.
   - `fetchRunningEntry` returns the running entry across all tasks.
   - `fetchRunningEntry` returns `nil` when no running entry exists.
4. Extend `WorkLogRepository` protocol with `addEntry`, `updateEntry`, and
   `fetchRunningEntry`.
5. Convert `InMemoryWorkLogRepository` from `struct` to `actor` and implement all three
   new methods.
6. Update `WorkLogRepositoryStub` to satisfy the new protocol methods with configurable
   `Result` return values.

### Slice 3 — `WorkLogTrackingService`

7. Create `WorkLogTrackingService.swift`:
   - `@Observable final class WorkLogTrackingService`
   - `private(set) var runningEntry: WorkLogEntry?`
   - `var isTracking: Bool { runningEntry != nil }`
   - `func start(_ entry: WorkLogEntry)`
   - `func stop()`
8. Update `TimeTrackerApp` to create `WorkLogTrackingService`, bootstrap it by calling
   `fetchRunningEntry()` on the repository, and inject it into the environment via
   `.environment(trackingService)`.

### Slice 4 — ViewModel tracking logic

9. Write failing tests for `WorkLogListViewModel` tracking behaviour:
   - `isTracking` delegates to `trackingService.isTracking`.
   - `isShowingTrackingError` is `true` when `trackingError` is non-nil.
   - `startTracking(for:)` stops any running entry via `updateEntry` (with `endedAt =
     .now` and `updatedAt = .now`) and calls `trackingService.stop()` when one is found.
   - `startTracking(for:)` calls `addEntry` with an entry where `startedAt ≈ .now` and
     `isRunning == true`, then calls `trackingService.start(newEntry)`.
   - `startTracking(for:)` reloads the entries list on success.
   - `startTracking(for:)` sets `trackingError` (not `loadingState`) on failure.
   - `stopTracking()` calls `updateEntry` with `endedAt = .now` and `updatedAt = .now`,
     then calls `trackingService.stop()`.
   - `stopTracking()` reloads the entries list on success.
   - `stopTracking()` sets `trackingError` (not `loadingState`) on failure.
   - `stopTracking()` is a no-op when `trackingService.isTracking` is `false`.
10. Update `WorkLogListViewModel` initialiser to accept `WorkLogTrackingService`.
11. Implement `isTracking`, `trackingError`, `isShowingTrackingError`, `startTracking`,
    and `stopTracking`.

### Slice 5 — UI: Play / Stop toolbar button + error alert

12. Add a `ToolbarItem(placement: .primaryAction)` to `WorkLogListView`:
    - `play.fill` when `!viewModel.isTracking`, `stop.fill` when `viewModel.isTracking`.
    - Calls `startTracking` or `stopTracking` via `Task { await … }`.
    - Disabled while `viewModel.isLoading`.
    - Localized accessibility labels: `"work-log-list.toolbar.start"` / `"Start
      Tracking"` and `"work-log-list.toolbar.stop"` / `"Stop Tracking"`.
13. Add a `.alert` to `WorkLogListView`:
    - Title: `"work-log-list.tracking-error.title"` / `"Tracking Error"`.
    - Message: `viewModel.trackingError ?? ""`.
    - Single OK button that sets `viewModel.trackingError = nil`.

#### Slice 5 execution notes

- Prefer a small private toolbar button builder (or computed view) in `WorkLogListView` if it improves readability, but avoid introducing a second ViewModel or new service.
- Keep the button action logic in the view layer only: `Task { await viewModel.startTracking(for: taskID) }` and `Task { await viewModel.stopTracking() }`.
- The alert dismiss action should clear `viewModel.trackingError` by setting it to `nil`; no retry button is added in this slice.
- Because there are currently no dedicated SwiftUI view tests in the project, Slice 5 should use TDD by first adding focused tests around any extracted presentation helpers/state that make the UI behavior explicit, then implement the SwiftUI wiring and verify with build/tests.
- The toolbar button state is task-specific: it shows Stop only when `trackingService.runningEntry?.taskID == taskID` for the currently selected work-log view; otherwise it shows Play, even if another task is being tracked globally.
- Do not broaden scope into elapsed timers, persistence/reactivity changes, or task-selection state promotion beyond this local toolbar behavior.
- Shared `WorkLogListViewModel` test support (for example `WorkLogRepositoryStub`) may be moved into a dedicated support file when reused across multiple test files, and unused test-double state should be removed when confirmed unnecessary.
- Small single-use view helpers in `WorkLogListView` may be inlined when that improves readability, and `WorkLogListViewModel` tests should stay grouped in a single file using smaller suites such as `DerivedState`, `TrackingTaskState`, `TrackingActionInFlight`, `StartTracking`, and `StopTracking` when helpful.
- Prevent tracking-action reentrancy in the `WorkLogListViewModel` itself with a dedicated in-flight flag (separate from `isLoading`), and disable the toolbar button while either loading entries or a tracking mutation is in flight.
- If auto-stopping the previously running entry succeeds but creating the new entry fails, update `trackingService` to reflect the persisted stopped state so repository and in-memory tracking state remain consistent.

---

## Checkpoints

### Slice 1 — Domain: `isRunning`
- [x] Write `isRunning` tests for `WorkLogEntry`.
- [x] Add `isRunning` computed property to `WorkLogEntry`.

### Slice 2 — Repository write operations
- [x] Write `addEntry`, `updateEntry`, and `fetchRunningEntry` tests.
- [x] Extend `WorkLogRepository` protocol.
- [x] Convert `InMemoryWorkLogRepository` to `actor` and implement all new methods.
- [x] Update `WorkLogRepositoryStub`.

### Slice 3 — `WorkLogTrackingService`
- [x] Create `WorkLogTrackingService`.
- [x] Bootstrap and inject in `TimeTrackerApp`.

### Slice 4 — ViewModel tracking logic
- [x] Write tracking tests for `WorkLogListViewModel`.
- [x] Update `WorkLogListViewModel` with `trackingService` dependency and tracking methods.

### Slice 5 — UI
- [ ] Add Play / Stop toolbar button to `WorkLogListView`.
- [ ] Add tracking error alert to `WorkLogListView`.

---

## Testing Strategy

### Domain
- `isRunning` is `true` when `endedAt` is `nil`, `false` otherwise.

### Persistence
- `addEntry` — appears in subsequent `fetchEntries` for the same `taskID`.
- `addEntry` — creates a new bucket for an unknown `taskID`.
- `addEntry` — does not affect other `taskID`s.
- `updateEntry` — replaces the original entry in the store.
- `updateEntry` — throws `.entryNotFound` for an unknown ID.
- `fetchRunningEntry` — returns the running entry across all tasks.
- `fetchRunningEntry` — returns `nil` when none exists.

### Presentation
- `isTracking` delegates to `trackingService.isTracking`.
- `isShowingTrackingError` reflects `trackingError != nil`.
- `startTracking` stops any previously running entry before starting a new one.
- `startTracking` calls `trackingService.stop()` then `trackingService.start(newEntry)`.
- `startTracking` reloads the list on success.
- `startTracking` sets `trackingError` (not `loadingState`) on failure.
- `stopTracking` sets `endedAt` and `updatedAt` to `.now` on the running entry.
- `stopTracking` calls `trackingService.stop()` on success.
- `stopTracking` reloads the list on success.
- `stopTracking` sets `trackingError` (not `loadingState`) on failure.
- `stopTracking` is a no-op when not tracking.

---

## Deferred For Later

- A live elapsed-time counter while tracking is active.
- Confirmation dialog before stopping tracking.
- A Retry button on the tracking error alert.
- Optional description field at stop time (`description` is always `nil` for now).
- Editing or deleting existing work log entries.
- Removing seeded work log data from `TimeTrackerApp` once the add-entry UI exists.
- SwiftData / CloudKit persistence.
- Promoting `selectedTaskID` to a shared environment object / store.
- Full `.xcstrings` localisation catalogue.
