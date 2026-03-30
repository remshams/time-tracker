import Foundation

@Observable
@MainActor
final class WorkLogListViewModel {
  private(set) var entries: [WorkLogEntry] = []
  private(set) var loadingState: LoadingState = .idle
  var trackingError: String?

  var isLoading: Bool { loadingState.isLoading }
  var isLoaded: Bool { loadingState.isLoaded }
  var errorMessage: String? { loadingState.errorMessage }
  var isTracking: Bool { trackingService.isTracking }
  var isShowingTrackingError: Bool { trackingError != nil }

  private let repository: any WorkLogRepository
  private let trackingService: WorkLogTrackingService

  init(
    repository: any WorkLogRepository,
    trackingService: WorkLogTrackingService = WorkLogTrackingService()
  ) {
    self.repository = repository
    self.trackingService = trackingService
  }

  func loadEntries(for taskID: WorkTask.ID) async {
    entries = []
    loadingState = .loading

    do {
      entries = try await repository.fetchEntries(for: taskID)
      loadingState = .loaded
    } catch {
      loadingState = .failed(
        String(
          localized: "work-log-list.error.message",
          defaultValue: "Failed to load work logs."))
    }
  }

  func startTracking(for taskID: WorkTask.ID) async {
    // Stop any currently-running entry before starting a new one. Both the
    // repository write and the in-memory service update are deferred until
    // after the new entry's addEntry succeeds, so a failure on either write
    // leaves the service state unchanged.
    let previousEntry = trackingService.runningEntry

    if let runningEntry = previousEntry {
      let updatedEntry: WorkLogEntry
      do {
        updatedEntry = try runningEntry.ended()
      } catch {
        trackingError = String(
          localized: "work-log-list.tracking.start-error",
          defaultValue: "Failed to start tracking.")
        return
      }

      do {
        try await repository.updateEntry(updatedEntry)
      } catch {
        // Intentionally leave trackingService running — the repository update
        // failed, so the running entry is still active. The user can retry.
        trackingError = String(
          localized: "work-log-list.tracking.start-error",
          defaultValue: "Failed to start tracking.")
        return
      }
    }

    let now = Date.now
    let newEntry: WorkLogEntry
    do {
      newEntry = try WorkLogEntry(taskID: taskID, startedAt: now, addedAt: now, updatedAt: now)
    } catch {
      trackingError = String(
        localized: "work-log-list.tracking.start-error",
        defaultValue: "Failed to start tracking.")
      return
    }

    do {
      try await repository.addEntry(newEntry)
    } catch {
      // addEntry failed: roll back the in-memory stop so the service still
      // reflects the old running entry (the repository update already succeeded,
      // but the service has not been mutated yet at this point).
      trackingError = String(
        localized: "work-log-list.tracking.start-error",
        defaultValue: "Failed to start tracking.")
      return
    }

    // Both writes succeeded — now update in-memory state atomically.
    if previousEntry != nil {
      trackingService.stop()
    }
    trackingService.start(newEntry)
    // NOTE: Only the newly-started task's entry list is reloaded here.
    // If the auto-stopped entry belonged to a different task, that task's
    // list view will remain stale until it is next focused.
    await loadEntries(for: taskID)
  }

  func stopTracking() async {
    guard let runningEntry = trackingService.runningEntry else { return }

    let updatedEntry: WorkLogEntry
    do {
      updatedEntry = try runningEntry.ended()
    } catch {
      trackingError = String(
        localized: "work-log-list.tracking.stop-error",
        defaultValue: "Failed to stop tracking.")
      return
    }

    do {
      try await repository.updateEntry(updatedEntry)
    } catch {
      trackingError = String(
        localized: "work-log-list.tracking.stop-error",
        defaultValue: "Failed to stop tracking.")
      return
    }

    trackingService.stop()
    await loadEntries(for: runningEntry.taskID)
  }
}
