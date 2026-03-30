enum WorkLogRepositoryError: Error, Equatable {
  case entryNotFound
  case runningEntryAlreadyExists
}

protocol WorkLogRepository: Sendable {
  func fetchEntries(for taskID: WorkTask.ID) async throws -> [WorkLogEntry]
  func fetchRunningEntry() async throws -> WorkLogEntry?
  func addEntry(_ entry: WorkLogEntry) async throws
  /// Updates an existing entry in place. The entry's `taskID` must match the task under
  /// which it was originally stored. Throws `WorkLogRepositoryError.entryNotFound` if no
  /// entry with the given `id` exists in the store.
  func updateEntry(_ entry: WorkLogEntry) async throws
}
