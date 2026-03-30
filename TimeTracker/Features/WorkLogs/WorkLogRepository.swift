enum WorkLogRepositoryError: Error, Equatable {
  case entryNotFound
}

protocol WorkLogRepository: Sendable {
  func fetchEntries(for taskID: WorkTask.ID) async throws -> [WorkLogEntry]
  func fetchRunningEntry() async throws -> WorkLogEntry?
  func addEntry(_ entry: WorkLogEntry) async throws
  func updateEntry(_ entry: WorkLogEntry) async throws
}
