import Foundation

@testable import TimeTracker

enum TestFactories {
  static let anyTaskID: WorkTask.ID = UUID(uuidString: "00000000-0000-0000-0000-000000000001")!
  static let anyTaskTitle = "Any task"

  static func makeTask(id: WorkTask.ID = .init(), title: String, description: String? = nil) -> WorkTask {
    // swiftlint:disable:next force_try
    try! WorkTask(id: id, title: title, description: description)
  }

  static func makeWorkLogEntry(
    id: UUID = .init(),
    taskID: WorkTask.ID,
    description: String? = nil,
    startedAt: Date = Date(timeIntervalSince1970: 1_700_000_000),
    addedAt: Date = Date(timeIntervalSince1970: 1_700_000_060),
    // Defaults to a completed entry so duration is non-nil in most fixture scenarios.
    endedAt: Date? = Date(timeIntervalSince1970: 1_700_000_360),
    updatedAt: Date = Date(timeIntervalSince1970: 1_700_000_420)
  ) -> WorkLogEntry {
    // swiftlint:disable:next force_try
    try! WorkLogEntry(
      id: id,
      taskID: taskID,
      description: description,
      startedAt: startedAt,
      addedAt: addedAt,
      endedAt: endedAt,
      updatedAt: updatedAt)
  }

  /// Returns a work log entry with no end time (i.e. currently running).
  /// All timestamps are internally consistent; callers need not supply any dates.
  static func makeRunningWorkLogEntry(
    id: UUID = .init(),
    taskID: WorkTask.ID,
    description: String? = nil
  ) -> WorkLogEntry {
    let startedAt = Date(timeIntervalSince1970: 1_700_000_000)
    let addedAt = Date(timeIntervalSince1970: 1_700_000_060)
    // swiftlint:disable:next force_try
    return try! WorkLogEntry(
      id: id,
      taskID: taskID,
      description: description,
      startedAt: startedAt,
      addedAt: addedAt,
      endedAt: nil,
      updatedAt: addedAt)
  }
}
