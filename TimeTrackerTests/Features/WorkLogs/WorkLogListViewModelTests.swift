import Foundation
import Testing

@testable import TimeTracker

@Suite struct WorkLogListViewModelTests {
  @Suite @MainActor struct LoadEntries {
    @Test func loadsEntriesFromRepository() async {
      let task = TestFactories.makeTask()
      let entries = [
        TestFactories.makeWorkLogEntry(taskID: task.id, description: "Initial architecture"),
        TestFactories.makeWorkLogEntry(taskID: task.id, description: "Draft implementation"),
      ]
      let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(fetchEntriesResult: .success(entries)))

      await viewModel.loadEntries(for: task.id)

      #expect(viewModel.entries == entries)
      #expect(viewModel.errorMessage == nil)
      #expect(!viewModel.isLoading)
    }

    @Test func exposesEmptyEntriesBeforeLoad() {
      let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(fetchEntriesResult: .success([])))

      #expect(viewModel.entries.isEmpty)
    }

    @Test func setsErrorMessageWhenLoadingFails() async {
      let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(
          fetchEntriesResult: .failure(WorkLogRepositoryStubError.fetchFailed)))

      await viewModel.loadEntries(for: TestFactories.anyTaskID)

      #expect(viewModel.entries.isEmpty)
      #expect(viewModel.errorMessage == "Failed to load work logs.")
      #expect(!viewModel.isLoading)
    }

    @Test func clearsEntriesWhenSubsequentLoadFails() async {
      let task = TestFactories.makeTask()
      let entries = [TestFactories.makeWorkLogEntry(taskID: task.id)]
      let stub = WorkLogRepositoryStub(fetchEntriesResult: .success(entries))
      let viewModel = WorkLogListViewModel(repository: stub)

      await viewModel.loadEntries(for: task.id)
      #expect(viewModel.entries == entries)

      stub.fetchEntriesResult = .failure(WorkLogRepositoryStubError.fetchFailed)
      await viewModel.loadEntries(for: task.id)

      #expect(viewModel.entries.isEmpty)
      #expect(viewModel.errorMessage == "Failed to load work logs.")
    }

    @Test func isNotLoadedBeforeFirstLoad() {
      let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(fetchEntriesResult: .success([])))

      #expect(!viewModel.isLoaded)
    }

    @Test func isLoadedAfterSuccessfulLoad() async {
      let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(fetchEntriesResult: .success([])))

      await viewModel.loadEntries(for: TestFactories.anyTaskID)

      #expect(viewModel.isLoaded)
    }

    @Test func isNotLoadedAfterFailedLoad() async {
      let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(
          fetchEntriesResult: .failure(WorkLogRepositoryStubError.fetchFailed)))

      await viewModel.loadEntries(for: TestFactories.anyTaskID)

      #expect(!viewModel.isLoaded)
    }
  }

  @Suite @MainActor struct Tracking {
    @Test func isTrackingDelegatesToTrackingService() {
      let trackingService = WorkLogTrackingService()
      let runningEntry = TestFactories.makeRunningWorkLogEntry(taskID: TestFactories.anyTaskID)
      trackingService.start(runningEntry)
      let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(fetchEntriesResult: .success([])),
        trackingService: trackingService)

      #expect(viewModel.isTracking)
    }

    @Test func isShowingTrackingErrorIsTrueWhenTrackingErrorIsSet() {
      let viewModel = WorkLogListViewModelTests.makeViewModel()

      viewModel.trackingError = "some error"

      #expect(viewModel.isShowingTrackingError)
    }

    @Test func isShowingTrackingErrorIsFalseWhenTrackingErrorIsNil() {
      let viewModel = WorkLogListViewModelTests.makeViewModel()

      #expect(!viewModel.isShowingTrackingError)
    }

    @Test func startTrackingAddsNewRunningEntry() async {
      let taskID = TestFactories.anyTaskID
      let stub = WorkLogRepositoryStub(fetchEntriesResult: .success([]))
      let viewModel = WorkLogListViewModel(repository: stub)

      await viewModel.startTracking(for: taskID)

      #expect(stub.lastAddedEntry?.isRunning == true)
      #expect(stub.lastAddedEntry?.taskID == taskID)
    }

    @Test func startTrackingStartsTrackingService() async {
      let taskID = TestFactories.anyTaskID
      let trackingService = WorkLogTrackingService()
      let stub = WorkLogRepositoryStub(fetchEntriesResult: .success([]))
      let viewModel = WorkLogListViewModel(repository: stub, trackingService: trackingService)

      await viewModel.startTracking(for: taskID)

      #expect(trackingService.isTracking)
    }

    @Test func startTrackingStopsRunningEntryBeforeStartingNewOne() async {
      let existingTaskID = UUID()
      let newTaskID = UUID()
      let existingEntry = TestFactories.makeRunningWorkLogEntry(taskID: existingTaskID)
      let trackingService = WorkLogListViewModelTests.makeTrackingService(tracking: existingEntry)
      let stub = WorkLogRepositoryStub(fetchEntriesResult: .success([]))
      let viewModel = WorkLogListViewModelTests.makeViewModel(stub: stub, trackingService: trackingService)

      await viewModel.startTracking(for: newTaskID)

      #expect(stub.lastUpdatedEntry?.id == existingEntry.id)
      #expect(stub.lastUpdatedEntry?.endedAt != nil)
      #expect(stub.lastUpdatedEntry?.updatedAt != nil)
    }

    @Test func startTrackingStopsTrackingServiceBeforeStartingNew() async {
      let existingTaskID = UUID()
      let newTaskID = UUID()
      let existingEntry = TestFactories.makeRunningWorkLogEntry(taskID: existingTaskID)
      let trackingService = WorkLogListViewModelTests.makeTrackingService(tracking: existingEntry)
      let stub = WorkLogRepositoryStub(fetchEntriesResult: .success([]))
      let viewModel = WorkLogListViewModelTests.makeViewModel(stub: stub, trackingService: trackingService)

      await viewModel.startTracking(for: newTaskID)

      #expect(trackingService.runningEntry?.taskID == newTaskID)
    }

    @Test func startTrackingReloadsEntriesOnSuccess() async {
      let taskID = TestFactories.anyTaskID
      let entries = [TestFactories.makeWorkLogEntry(taskID: taskID)]
      let stub = WorkLogRepositoryStub(fetchEntriesResult: .success(entries))
      let viewModel = WorkLogListViewModel(repository: stub)

      await viewModel.startTracking(for: taskID)

      #expect(viewModel.entries == entries)
      #expect(stub.lastFetchedTaskID == taskID)
    }

    @Test func startTrackingSetsTrackingErrorOnAddFailure() async {
      let taskID = TestFactories.anyTaskID
      let stub = WorkLogRepositoryStub(fetchEntriesResult: .success([]))
      stub.addEntryResult = .failure(WorkLogRepositoryStubError.writeFailed)
      let viewModel = WorkLogListViewModel(repository: stub)

      await viewModel.startTracking(for: taskID)

      #expect(viewModel.trackingError != nil)
      #expect(viewModel.errorMessage == nil)
    }

    @Test func startTrackingDoesNotAddNewEntryWhenAutoStopFails() async {
      let existingEntry = TestFactories.makeRunningWorkLogEntry(taskID: UUID())
      let trackingService = WorkLogTrackingService()
      trackingService.start(existingEntry)
      let stub = WorkLogRepositoryStub(fetchEntriesResult: .success([]))
      stub.updateEntryResult = .failure(WorkLogRepositoryStubError.writeFailed)
      let viewModel = WorkLogListViewModel(repository: stub, trackingService: trackingService)

      await viewModel.startTracking(for: TestFactories.anyTaskID)

      #expect(stub.lastAddedEntry == nil)
      #expect(viewModel.trackingError != nil)
      #expect(trackingService.runningEntry?.id == existingEntry.id)
    }

    @Test func startTrackingDoesNotStartTrackingServiceWhenAddEntryFails() async {
      let existingEntry = TestFactories.makeRunningWorkLogEntry(taskID: UUID())
      let trackingService = WorkLogTrackingService()
      trackingService.start(existingEntry)
      let newTaskID = UUID()
      let stub = WorkLogRepositoryStub(fetchEntriesResult: .success([]))
      // updateEntry (auto-stop) succeeds, addEntry (new entry) fails
      stub.addEntryResult = .failure(WorkLogRepositoryStubError.writeFailed)
      let viewModel = WorkLogListViewModel(repository: stub, trackingService: trackingService)

      await viewModel.startTracking(for: newTaskID)

      #expect(stub.lastUpdatedEntry?.id == existingEntry.id)
      #expect(stub.lastUpdatedEntry?.endedAt != nil)
      #expect(stub.lastAddedEntry?.taskID == newTaskID)
      #expect(viewModel.trackingError != nil)
      #expect(trackingService.runningEntry?.id == existingEntry.id)
    }

    @Test func stopTrackingUpdatesRunningEntryWithEndedAt() async {
      let taskID = TestFactories.anyTaskID
      let runningEntry = TestFactories.makeRunningWorkLogEntry(taskID: taskID)
      let trackingService = WorkLogListViewModelTests.makeTrackingService(tracking: runningEntry)
      let stub = WorkLogRepositoryStub(fetchEntriesResult: .success([]))
      let viewModel = WorkLogListViewModelTests.makeViewModel(stub: stub, trackingService: trackingService)

      await viewModel.stopTracking()

      #expect(stub.lastUpdatedEntry?.endedAt != nil)
      #expect(stub.lastUpdatedEntry?.id == runningEntry.id)
    }

    @Test func stopTrackingStopsTrackingService() async {
      let taskID = TestFactories.anyTaskID
      let runningEntry = TestFactories.makeRunningWorkLogEntry(taskID: taskID)
      let trackingService = WorkLogListViewModelTests.makeTrackingService(tracking: runningEntry)
      let stub = WorkLogRepositoryStub(fetchEntriesResult: .success([]))
      let viewModel = WorkLogListViewModelTests.makeViewModel(stub: stub, trackingService: trackingService)

      await viewModel.stopTracking()

      #expect(!trackingService.isTracking)
    }

    @Test func stopTrackingReloadsEntriesOnSuccess() async {
      let taskID = TestFactories.anyTaskID
      let entries = [TestFactories.makeWorkLogEntry(taskID: taskID)]
      let runningEntry = TestFactories.makeRunningWorkLogEntry(taskID: taskID)
      let trackingService = WorkLogListViewModelTests.makeTrackingService(tracking: runningEntry)
      let stub = WorkLogRepositoryStub(fetchEntriesResult: .success(entries))
      let viewModel = WorkLogListViewModelTests.makeViewModel(stub: stub, trackingService: trackingService)

      await viewModel.stopTracking()

      #expect(viewModel.entries == entries)
      #expect(stub.lastFetchedTaskID == taskID)
    }

    @Test func stopTrackingSetsTrackingErrorOnUpdateFailure() async {
      let taskID = TestFactories.anyTaskID
      let runningEntry = TestFactories.makeRunningWorkLogEntry(taskID: taskID)
      let trackingService = WorkLogListViewModelTests.makeTrackingService(tracking: runningEntry)
      let stub = WorkLogRepositoryStub(fetchEntriesResult: .success([]))
      stub.updateEntryResult = .failure(WorkLogRepositoryStubError.writeFailed)
      let viewModel = WorkLogListViewModelTests.makeViewModel(stub: stub, trackingService: trackingService)

      await viewModel.stopTracking()

      #expect(viewModel.trackingError != nil)
      #expect(viewModel.errorMessage == nil)
      #expect(trackingService.isTracking)
      #expect(trackingService.runningEntry?.id == runningEntry.id)
    }

    @Test func stopTrackingIsNoOpWhenNotTracking() async {
      let stub = WorkLogRepositoryStub(fetchEntriesResult: .success([]))
      let viewModel = WorkLogListViewModelTests.makeViewModel(stub: stub)

      await viewModel.stopTracking()

      #expect(stub.lastUpdatedEntry == nil)
    }

    @Test func startTrackingClearsStaleTrackingError() async {
      let stub = WorkLogRepositoryStub(fetchEntriesResult: .success([]))
      let viewModel = WorkLogListViewModelTests.makeViewModel(stub: stub)
      viewModel.trackingError = "stale error"

      await viewModel.startTracking(for: TestFactories.anyTaskID)

      #expect(viewModel.trackingError == nil)
    }

    @Test func stopTrackingClearsStaleTrackingError() async {
      let taskID = TestFactories.anyTaskID
      let runningEntry = TestFactories.makeRunningWorkLogEntry(taskID: taskID)
      let trackingService = WorkLogListViewModelTests.makeTrackingService(tracking: runningEntry)
      let stub = WorkLogRepositoryStub(fetchEntriesResult: .success([]))
      let viewModel = WorkLogListViewModelTests.makeViewModel(stub: stub, trackingService: trackingService)
      viewModel.trackingError = "stale error"

      await viewModel.stopTracking()

      #expect(viewModel.trackingError == nil)
    }

  }
}

// MARK: - Helpers

extension WorkLogListViewModelTests {
  @MainActor
  private static func makeViewModel(
    stub: WorkLogRepositoryStub,
    trackingService: WorkLogTrackingService = WorkLogTrackingService()
  ) -> WorkLogListViewModel {
    WorkLogListViewModel(repository: stub, trackingService: trackingService)
  }

  @MainActor
  private static func makeViewModel() -> WorkLogListViewModel {
    WorkLogListViewModel(
      repository: WorkLogRepositoryStub(fetchEntriesResult: .success([])))
  }

  @MainActor
  private static func makeTrackingService(tracking entry: WorkLogEntry) -> WorkLogTrackingService {
    let service = WorkLogTrackingService()
    service.start(entry)
    return service
  }
}

// MARK: - Test doubles

@MainActor
final class WorkLogRepositoryStub: WorkLogRepository {
  var fetchEntriesResult: Result<[WorkLogEntry], Error>
  var fetchRunningEntryResult: Result<WorkLogEntry?, Error> = .success(nil)
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
    try fetchRunningEntryResult.get()
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
