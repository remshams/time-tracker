actor InMemoryWorkLogRepository: WorkLogRepository {
  private var entriesByTaskID: [WorkTask.ID: [WorkLogEntry]]

  init(entriesByTaskID: [WorkTask.ID: [WorkLogEntry]] = [:]) {
    self.entriesByTaskID = entriesByTaskID
  }

  func fetchEntries(for taskID: WorkTask.ID) async throws -> [WorkLogEntry] {
    entriesByTaskID[taskID] ?? []
  }

  func fetchRunningEntry() async throws -> WorkLogEntry? {
    entriesByTaskID.values.joined().first(where: \.isRunning)
  }

  func addEntry(_ entry: WorkLogEntry) async throws {
    if entry.isRunning, entriesByTaskID.values.joined().contains(where: \.isRunning) {
      throw WorkLogRepositoryError.runningEntryAlreadyExists
    }
    entriesByTaskID[entry.taskID, default: []].append(entry)
  }

  func updateEntry(_ entry: WorkLogEntry) async throws {
    guard let index = entriesByTaskID[entry.taskID]?.firstIndex(where: { $0.id == entry.id }) else {
      throw WorkLogRepositoryError.entryNotFound
    }
    entriesByTaskID[entry.taskID]?[index] = entry
  }
}
