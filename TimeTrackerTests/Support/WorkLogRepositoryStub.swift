import Foundation

@testable import TimeTracker

@MainActor
final class WorkLogRepositoryStub: WorkLogRepository {
  var fetchEntriesResult: Result<[WorkLogEntry], Error>
  var addEntryResult: Result<Void, Error> = .success(())
  var updateEntryResult: Result<Void, Error> = .success(())
  var lastAddedEntry: WorkLogEntry?
  var lastUpdatedEntry: WorkLogEntry?
  var lastFetchedTaskID: WorkTask.ID?

  init(fetchEntriesResult: Result<[WorkLogEntry], Error>) {
    self.fetchEntriesResult = fetchEntriesResult
  }

  func fetchEntries(for taskID: WorkTask.ID) async throws -> [WorkLogEntry] {
    lastFetchedTaskID = taskID
    return try fetchEntriesResult.get()
  }

  func fetchRunningEntry() async throws -> WorkLogEntry? {
    nil
  }

  func addEntry(_ entry: WorkLogEntry) async throws {
    lastAddedEntry = entry
    try addEntryResult.get()
  }

  func updateEntry(_ entry: WorkLogEntry) async throws {
    lastUpdatedEntry = entry
    try updateEntryResult.get()
  }
}

enum WorkLogRepositoryStubError: Error, Sendable {
  case fetchFailed
  case writeFailed
}
