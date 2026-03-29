struct InMemoryWorkLogRepository: WorkLogRepository, Sendable {
  private let entriesByTaskID: [WorkTask.ID: [WorkLogEntry]]

  init(entriesByTaskID: [WorkTask.ID: [WorkLogEntry]] = [:]) {
    self.entriesByTaskID = entriesByTaskID
  }

  func fetchEntries(for taskID: WorkTask.ID) async throws -> [WorkLogEntry] {
    entriesByTaskID[taskID] ?? []
  }
}
