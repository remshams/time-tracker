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

  @Suite @MainActor struct DerivedState {
    private static let staleTrackingError = "stale error"

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

    @Test func startTrackingClearsStaleTrackingError() async {
      let stub = WorkLogRepositoryStub(fetchEntriesResult: .success([]))
      let viewModel = WorkLogListViewModelTests.makeViewModel(stub: stub)
      viewModel.trackingError = Self.staleTrackingError

      await viewModel.startTracking(for: TestFactories.anyTaskID)

      #expect(viewModel.trackingError == nil)
    }

    @Test func stopTrackingClearsStaleTrackingError() async {
      let taskID = TestFactories.anyTaskID
      let runningEntry = TestFactories.makeRunningWorkLogEntry(taskID: taskID)
      let trackingService = WorkLogListViewModelTests.makeTrackingService(tracking: runningEntry)
      let stub = WorkLogRepositoryStub(fetchEntriesResult: .success([]))
      let viewModel = WorkLogListViewModelTests.makeViewModel(stub: stub, trackingService: trackingService)
      viewModel.trackingError = Self.staleTrackingError

      await viewModel.stopTracking()

      #expect(viewModel.trackingError == nil)
    }
  }

  @Suite @MainActor struct TrackingTaskState {
    @Test func isTrackingTaskIsTrueWhenRunningEntryMatchesTask() {
      let taskID = TestFactories.anyTaskID
      let trackingService = WorkLogTrackingService()
      trackingService.start(TestFactories.makeRunningWorkLogEntry(taskID: taskID))
      let stub = WorkLogRepositoryStub(fetchEntriesResult: .success([]))
      let viewModel = WorkLogListViewModel(repository: stub, trackingService: trackingService)

      #expect(viewModel.isTrackingTask(taskID))
    }

    @Test func isTrackingTaskIsFalseWhenAnotherTaskIsRunning() {
      let runningTaskID = UUID()
      let selectedTaskID = UUID()
      let trackingService = WorkLogTrackingService()
      trackingService.start(TestFactories.makeRunningWorkLogEntry(taskID: runningTaskID))
      let stub = WorkLogRepositoryStub(fetchEntriesResult: .success([]))
      let viewModel = WorkLogListViewModel(repository: stub, trackingService: trackingService)

      #expect(!viewModel.isTrackingTask(selectedTaskID))
    }

    @Test func isTrackingTaskIsFalseWhenNothingIsRunning() {
      let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(fetchEntriesResult: .success([])))

      #expect(!viewModel.isTrackingTask(TestFactories.anyTaskID))
    }
  }

  @Suite @MainActor struct StartTracking {
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
      #expect(stub.lastUpdatedEntry?.taskID == existingTaskID)
      #expect(stub.lastUpdatedEntry?.endedAt != nil)
      #expect(stub.lastUpdatedEntry?.updatedAt == stub.lastUpdatedEntry?.endedAt)
      #expect(stub.lastAddedEntry?.taskID == newTaskID)
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
      #expect(!trackingService.isTracking)
      #expect(trackingService.runningEntry == nil)
    }
  }

  @Suite @MainActor struct TrackingActionInFlight {
    @Test func startTrackingResetsInFlightFlagAfterSuccess() async {
      let viewModel = WorkLogListViewModelTests.makeViewModel(
        stub: WorkLogRepositoryStub(fetchEntriesResult: .success([])))

      await viewModel.startTracking(for: TestFactories.anyTaskID)

      #expect(!viewModel.isTrackingActionInFlight)
    }

    @Test func startTrackingResetsInFlightFlagAfterFailure() async {
      let stub = WorkLogRepositoryStub(fetchEntriesResult: .success([]))
      stub.addEntryResult = .failure(WorkLogRepositoryStubError.writeFailed)
      let viewModel = WorkLogListViewModelTests.makeViewModel(stub: stub)

      await viewModel.startTracking(for: TestFactories.anyTaskID)

      #expect(!viewModel.isTrackingActionInFlight)
    }

    @Test func stopTrackingResetsInFlightFlagAfterSuccess() async {
      let taskID = TestFactories.anyTaskID
      let runningEntry = TestFactories.makeRunningWorkLogEntry(taskID: taskID)
      let trackingService = WorkLogListViewModelTests.makeTrackingService(tracking: runningEntry)
      let viewModel = WorkLogListViewModelTests.makeViewModel(
        stub: WorkLogRepositoryStub(fetchEntriesResult: .success([])),
        trackingService: trackingService)

      await viewModel.stopTracking()

      #expect(!viewModel.isTrackingActionInFlight)
    }

    @Test func stopTrackingResetsInFlightFlagAfterFailure() async {
      let taskID = TestFactories.anyTaskID
      let runningEntry = TestFactories.makeRunningWorkLogEntry(taskID: taskID)
      let trackingService = WorkLogListViewModelTests.makeTrackingService(tracking: runningEntry)
      let stub = WorkLogRepositoryStub(fetchEntriesResult: .success([]))
      stub.updateEntryResult = .failure(WorkLogRepositoryStubError.writeFailed)
      let viewModel = WorkLogListViewModelTests.makeViewModel(
        stub: stub,
        trackingService: trackingService)

      await viewModel.stopTracking()

      #expect(!viewModel.isTrackingActionInFlight)
    }

    @Test func startTrackingIsNoOpWhenCalledReentrantly() async {
      let taskID = TestFactories.anyTaskID
      let repository = WorkLogListBlockingTrackingActionRepository()
      let viewModel = WorkLogListViewModel(repository: repository)

      let firstCall = Task {
        await viewModel.startTracking(for: taskID)
      }

      await repository.waitForAddEntryToStart()
      #expect(viewModel.isTrackingActionInFlight)

      await viewModel.startTracking(for: taskID)

      #expect(await repository.addEntryCallCount == 1)
      #expect(await repository.fetchEntriesCallCount == 0)

      await repository.resumeAddEntry()
      await firstCall.value

      #expect(await repository.addEntryCallCount == 1)
      #expect(await repository.fetchEntriesCallCount == 1)
      #expect(!viewModel.isTrackingActionInFlight)
    }
  }

  @Suite @MainActor struct StopTracking {
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
  }

}

// MARK: - Helpers

private actor WorkLogListBlockingTrackingActionRepository: WorkLogRepository {
  private(set) var addEntryCallCount = 0
  private(set) var fetchEntriesCallCount = 0

  private var addEntryStartedContinuation: CheckedContinuation<Void, Never>?
  private var addEntryResumeContinuation: CheckedContinuation<Void, Never>?

  func fetchEntries(for taskID: WorkTask.ID) async throws -> [WorkLogEntry] {
    fetchEntriesCallCount += 1
    return []
  }

  func fetchRunningEntry() async throws -> WorkLogEntry? {
    nil
  }

  func addEntry(_ entry: WorkLogEntry) async throws {
    addEntryCallCount += 1
    addEntryStartedContinuation?.resume()
    addEntryStartedContinuation = nil

    await withCheckedContinuation { continuation in
      addEntryResumeContinuation = continuation
    }
  }

  func updateEntry(_ entry: WorkLogEntry) async throws {}

  func waitForAddEntryToStart() async {
    if addEntryCallCount > 0 { return }

    await withCheckedContinuation { continuation in
      addEntryStartedContinuation = continuation
    }
  }

  func resumeAddEntry() async {
    addEntryResumeContinuation?.resume()
    addEntryResumeContinuation = nil
  }
}

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
