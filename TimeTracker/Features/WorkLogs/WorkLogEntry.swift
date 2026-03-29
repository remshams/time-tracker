import Foundation

struct WorkLogEntry: Identifiable, Equatable, Sendable {
  enum ValidationError: Error, Equatable {
    case endedBeforeStarted
    case updatedBeforeAdded
  }

  nonisolated let id: UUID
  nonisolated let taskID: Task.ID
  nonisolated let description: String?
  nonisolated let startedAt: Date
  nonisolated let addedAt: Date
  nonisolated let endedAt: Date?
  nonisolated let updatedAt: Date

  nonisolated var duration: Duration? {
    guard let endedAt else {
      return nil
    }

    return .seconds(endedAt.timeIntervalSince(startedAt))
  }

  nonisolated init(
    id: UUID = UUID(),
    taskID: Task.ID,
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
