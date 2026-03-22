struct InMemoryWorkLogRepository: WorkLogRepository, Sendable {
  private let entriesByTaskID: [Task.ID: [WorkLogEntry]]

  init(entriesByTaskID: [Task.ID: [WorkLogEntry]] = [:]) {
    self.entriesByTaskID = entriesByTaskID
  }

  func fetchEntries(for taskID: Task.ID) async throws -> [WorkLogEntry] {
    entriesByTaskID[taskID] ?? []
  }
}
