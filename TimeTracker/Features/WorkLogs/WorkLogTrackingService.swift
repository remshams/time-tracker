import Foundation

@Observable
final class WorkLogTrackingService {
  private(set) var runningEntry: WorkLogEntry?

  var isTracking: Bool { runningEntry != nil }

  func start(_ entry: WorkLogEntry) {
    runningEntry = entry
  }

  func stop() {
    runningEntry = nil
  }
}
