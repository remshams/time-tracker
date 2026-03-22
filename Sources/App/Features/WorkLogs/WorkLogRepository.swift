protocol WorkLogRepository: Sendable {
  func fetchEntries(for taskID: Task.ID) async throws -> [WorkLogEntry]
}
