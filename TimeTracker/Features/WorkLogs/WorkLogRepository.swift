protocol WorkLogRepository: Sendable {
  func fetchEntries(for taskID: WorkTask.ID) async throws -> [WorkLogEntry]
}
