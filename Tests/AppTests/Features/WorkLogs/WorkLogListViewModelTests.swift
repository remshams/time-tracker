import Testing

@testable import App

@Suite @MainActor struct WorkLogListViewModelTests {
  @Suite @MainActor struct LoadEntries {
    @Test func loadsEntriesFromRepository() async {
      let task = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
      let entries = [
        TestFactories.makeWorkLogEntry(taskID: task.id, description: "Initial architecture"),
        TestFactories.makeWorkLogEntry(taskID: task.id, description: "Draft implementation"),
      ]
      let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(result: .success(entries)))

      await viewModel.loadEntries(for: task.id)

      #expect(viewModel.entries == entries)
      #expect(viewModel.errorMessage == nil)
      #expect(viewModel.isLoading == false)
    }

    @Test func exposesEmptyEntriesBeforeLoad() {
      let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(result: .success([])))

      #expect(viewModel.entries.isEmpty)
    }

    @Test func setsErrorMessageWhenLoadingFails() async {
      let task = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
      let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(result: .failure(WorkLogRepositoryStubError.fetchFailed)))

      await viewModel.loadEntries(for: task.id)

      #expect(viewModel.entries.isEmpty)
      #expect(viewModel.errorMessage == "Failed to load work logs.")
      #expect(viewModel.isLoading == false)
    }

    @Test func clearsEntriesWhenSubsequentLoadFails() async {
      let task = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
      let entries = [TestFactories.makeWorkLogEntry(taskID: task.id)]
      let stub = WorkLogRepositoryStub(result: .success(entries))
      let viewModel = WorkLogListViewModel(repository: stub)

      await viewModel.loadEntries(for: task.id)
      #expect(viewModel.entries == entries)

      stub.result = .failure(WorkLogRepositoryStubError.fetchFailed)
      await viewModel.loadEntries(for: task.id)

      #expect(viewModel.entries.isEmpty)
      #expect(viewModel.errorMessage == "Failed to load work logs.")
    }

    @Test func isNotLoadedBeforeFirstLoad() {
      let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(result: .success([])))

      #expect(viewModel.isLoaded == false)
    }

    @Test func isLoadedAfterSuccessfulLoad() async {
      let task = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
      let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(result: .success([])))

      await viewModel.loadEntries(for: task.id)

      #expect(viewModel.isLoaded == true)
    }

    @Test func isNotLoadedAfterFailedLoad() async {
      let task = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
      let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(result: .failure(WorkLogRepositoryStubError.fetchFailed)))

      await viewModel.loadEntries(for: task.id)

      #expect(viewModel.isLoaded == false)
    }
  }
}

// MARK: - Test doubles

private final class WorkLogRepositoryStub: WorkLogRepository, @unchecked Sendable {
  var result: Result<[WorkLogEntry], Error>

  init(result: Result<[WorkLogEntry], Error>) {
    self.result = result
  }

  func fetchEntries(for taskID: Task.ID) async throws -> [WorkLogEntry] {
    try result.get()
  }
}

private enum WorkLogRepositoryStubError: Error, Sendable {
  case fetchFailed
}
