import Foundation

@Observable
@MainActor
final class WorkLogTrackingService {
  private(set) var runningEntry: WorkLogEntry?

  var isTracking: Bool { runningEntry != nil }

  nonisolated init() {}

  func start(_ entry: WorkLogEntry) {
    runningEntry = entry
  }

  func stop() {
    runningEntry = nil
  }
}
