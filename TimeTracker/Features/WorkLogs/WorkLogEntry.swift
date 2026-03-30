import Foundation

struct WorkLogEntry: Identifiable, Equatable, Sendable {
  enum ValidationError: Error, Equatable {
    case endedBeforeStarted
    case updatedBeforeAdded
  }

  let id: UUID
  let taskID: WorkTask.ID
  let description: String?
  let startedAt: Date
  let addedAt: Date
  let endedAt: Date?
  let updatedAt: Date

  var isRunning: Bool { endedAt == nil }

  var duration: Duration? {
    guard let endedAt else {
      return nil
    }

    return .seconds(endedAt.timeIntervalSince(startedAt))
  }

  init(
    id: UUID = UUID(),
    taskID: WorkTask.ID,
    description: String? = nil,
    startedAt: Date,
    addedAt: Date = .now,
    endedAt: Date? = nil,
    updatedAt: Date = .now
  ) throws {
    if let endedAt, endedAt < startedAt {
      throw ValidationError.endedBeforeStarted
    }

    guard updatedAt >= addedAt else {
      throw ValidationError.updatedBeforeAdded
    }

    self.id = id
    self.taskID = taskID
    self.description = description
    self.startedAt = startedAt
    self.addedAt = addedAt
    self.endedAt = endedAt
    self.updatedAt = updatedAt
  }

  nonisolated static func == (lhs: WorkLogEntry, rhs: WorkLogEntry) -> Bool {
    lhs.id == rhs.id
      && lhs.taskID == rhs.taskID
      && lhs.description == rhs.description
      && lhs.startedAt == rhs.startedAt
      && lhs.addedAt == rhs.addedAt
      && lhs.endedAt == rhs.endedAt
      && lhs.updatedAt == rhs.updatedAt
  }
}

extension WorkLogEntry {
  /// Returns a copy of this entry with `endedAt` and `updatedAt` both set to `at`.
  func ended(at date: Date = .now) throws -> WorkLogEntry {
    try WorkLogEntry(
      id: id,
      taskID: taskID,
      description: description,
      startedAt: startedAt,
      addedAt: addedAt,
      endedAt: date,
      updatedAt: date)
  }
}
