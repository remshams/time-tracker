import Foundation
import Testing

@testable import TimeTracker

@Suite @MainActor struct WorkLogListViewModelTrackingTaskTests {
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
