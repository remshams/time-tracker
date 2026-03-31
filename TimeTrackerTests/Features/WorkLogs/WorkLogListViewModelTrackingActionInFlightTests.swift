import Foundation
import Testing

@testable import TimeTracker

@Suite @MainActor struct WorkLogListViewModelTrackingActionInFlightTests {
  @Test func startTrackingResetsInFlightFlagAfterSuccess() async {
    let viewModel = makeViewModel(stub: WorkLogRepositoryStub(fetchEntriesResult: .success([])))

    await viewModel.startTracking(for: TestFactories.anyTaskID)

    #expect(!viewModel.isTrackingActionInFlight)
  }

  @Test func startTrackingResetsInFlightFlagAfterFailure() async {
    let stub = WorkLogRepositoryStub(fetchEntriesResult: .success([]))
    stub.addEntryResult = .failure(WorkLogRepositoryStubError.writeFailed)
    let viewModel = makeViewModel(stub: stub)

    await viewModel.startTracking(for: TestFactories.anyTaskID)

    #expect(!viewModel.isTrackingActionInFlight)
  }

  @Test func stopTrackingResetsInFlightFlagAfterSuccess() async {
    let taskID = TestFactories.anyTaskID
    let runningEntry = TestFactories.makeRunningWorkLogEntry(taskID: taskID)
    let trackingService = makeTrackingService(tracking: runningEntry)
    let viewModel = makeViewModel(
      stub: WorkLogRepositoryStub(fetchEntriesResult: .success([])),
      trackingService: trackingService)

    await viewModel.stopTracking()

    #expect(!viewModel.isTrackingActionInFlight)
  }

  @Test func stopTrackingResetsInFlightFlagAfterFailure() async {
    let taskID = TestFactories.anyTaskID
    let runningEntry = TestFactories.makeRunningWorkLogEntry(taskID: taskID)
    let trackingService = makeTrackingService(tracking: runningEntry)
    let stub = WorkLogRepositoryStub(fetchEntriesResult: .success([]))
    stub.updateEntryResult = .failure(WorkLogRepositoryStubError.writeFailed)
    let viewModel = makeViewModel(stub: stub, trackingService: trackingService)

    await viewModel.stopTracking()

    #expect(!viewModel.isTrackingActionInFlight)
  }

  @Test func startTrackingIsNoOpWhenCalledReentrantly() async {
    let taskID = TestFactories.anyTaskID
    let repository = BlockingTrackingActionRepository()
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

// MARK: - Helpers

@MainActor
private func makeViewModel(
  stub: WorkLogRepositoryStub,
  trackingService: WorkLogTrackingService = WorkLogTrackingService()
) -> WorkLogListViewModel {
  WorkLogListViewModel(repository: stub, trackingService: trackingService)
}

@MainActor
private func makeTrackingService(tracking entry: WorkLogEntry) -> WorkLogTrackingService {
  let service = WorkLogTrackingService()
  service.start(entry)
  return service
}

actor BlockingTrackingActionRepository: WorkLogRepository {
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
