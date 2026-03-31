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

  func isTrackingTask(_ taskID: WorkTask.ID) -> Bool {
    trackingService.runningEntry?.taskID == taskID
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
    trackingError = nil
    let startTrackingErrorMessage = String(
      localized: "work-log-list.tracking.start-error",
      defaultValue: "Failed to start tracking.")
    let previousEntry = trackingService.runningEntry

    if let runningEntry = previousEntry {
      do {
        try await persistStop(of: runningEntry)
      } catch {
        trackingError = startTrackingErrorMessage
        return
      }
    }

    let now = Date.now
    let newEntry: WorkLogEntry
    do {
      newEntry = try WorkLogEntry(taskID: taskID, startedAt: now, addedAt: now, updatedAt: now)
    } catch {
      trackingError = startTrackingErrorMessage
      return
    }

    do {
      try await repository.addEntry(newEntry)
    } catch {
      trackingError = startTrackingErrorMessage
      return
    }

    if previousEntry != nil {
      trackingService.stop()
    }
    trackingService.start(newEntry)
    await loadEntries(for: taskID)
  }

  func stopTracking() async {
    trackingError = nil
    let stopTrackingErrorMessage = String(
      localized: "work-log-list.tracking.stop-error",
      defaultValue: "Failed to stop tracking.")
    guard let runningEntry = trackingService.runningEntry else { return }

    do {
      try await persistStop(of: runningEntry)
    } catch {
      trackingError = stopTrackingErrorMessage
      return
    }

    trackingService.stop()
    await loadEntries(for: runningEntry.taskID)
  }

  private func persistStop(of entry: WorkLogEntry) async throws {
    try await repository.updateEntry(try entry.ended())
  }
}
