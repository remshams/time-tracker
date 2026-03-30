import Testing

@testable import TimeTracker

@Suite struct WorkLogListViewModelTests {
  @Suite @MainActor struct LoadEntries {
    @Test func loadsEntriesFromRepository() async {
      let task = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
      let entries = [
        TestFactories.makeWorkLogEntry(taskID: task.id, description: "Initial architecture"),
        TestFactories.makeWorkLogEntry(taskID: task.id, description: "Draft implementation"),
      ]
      let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(fetchEntriesResult: .success(entries)))

      await viewModel.loadEntries(for: task.id)

      #expect(viewModel.entries == entries)
      #expect(viewModel.errorMessage == nil)
      #expect(viewModel.isLoading == false)
    }

    @Test func exposesEmptyEntriesBeforeLoad() {
      let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(fetchEntriesResult: .success([])))

      #expect(viewModel.entries.isEmpty)
    }

    @Test func setsErrorMessageWhenLoadingFails() async {
      let task = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
      let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(
          fetchEntriesResult: .failure(WorkLogRepositoryStubError.fetchFailed)))

      await viewModel.loadEntries(for: task.id)

      #expect(viewModel.entries.isEmpty)
      #expect(viewModel.errorMessage == "Failed to load work logs.")
      #expect(viewModel.isLoading == false)
    }

    @Test func clearsEntriesWhenSubsequentLoadFails() async {
      let task = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
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

      #expect(viewModel.isLoaded == false)
    }

    @Test func isLoadedAfterSuccessfulLoad() async {
      let task = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
      let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(fetchEntriesResult: .success([])))

      await viewModel.loadEntries(for: task.id)

      #expect(viewModel.isLoaded == true)
    }

    @Test func isNotLoadedAfterFailedLoad() async {
      let task = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
      let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(
          fetchEntriesResult: .failure(WorkLogRepositoryStubError.fetchFailed)))

      await viewModel.loadEntries(for: task.id)

      #expect(viewModel.isLoaded == false)
    }
  }
}

// MARK: - Test doubles

final class WorkLogRepositoryStub: WorkLogRepository, @unchecked Sendable {
  var fetchEntriesResult: Result<[WorkLogEntry], Error>
  var fetchRunningEntryResult: Result<WorkLogEntry?, Error> = .success(nil)
  var addEntryResult: Result<Void, Error> = .success(())
  var updateEntryResult: Result<Void, Error> = .success(())

  init(fetchEntriesResult: Result<[WorkLogEntry], Error>) {
    self.fetchEntriesResult = fetchEntriesResult
  }

  func fetchEntries(for taskID: WorkTask.ID) async throws -> [WorkLogEntry] {
    try fetchEntriesResult.get()
  }

  func fetchRunningEntry() async throws -> WorkLogEntry? {
    try fetchRunningEntryResult.get()
  }

  func addEntry(_ entry: WorkLogEntry) async throws {
    try addEntryResult.get()
  }

  func updateEntry(_ entry: WorkLogEntry) async throws {
    try updateEntryResult.get()
  }
}

enum WorkLogRepositoryStubError: Error, Sendable {
  case fetchFailed
  case writeFailed
}
