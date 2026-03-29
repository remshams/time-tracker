struct InMemoryWorkLogRepository: WorkLogRepository, Sendable {
  nonisolated let entriesByTaskID: [Task.ID: [WorkLogEntry]]

  nonisolated init(entriesByTaskID: [Task.ID: [WorkLogEntry]] = [:]) {
    self.entriesByTaskID = entriesByTaskID
  }

  func fetchEntries(for taskID: Task.ID) async throws -> [WorkLogEntry] {
    entriesByTaskID[taskID] ?? []
  }
}
