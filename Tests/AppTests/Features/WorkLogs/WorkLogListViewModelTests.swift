import Testing

@testable import App

@MainActor
@Test func workLogListViewModelLoadsEntriesFromRepository() async {
    let task = TestFactories.makeTask(title: "Write project plan")
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

@MainActor
@Test func workLogListViewModelExposesEmptyEntriesBeforeLoad() {
    let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(result: .success([])))

    #expect(viewModel.entries.isEmpty)
}

@MainActor
@Test func workLogListViewModelStoresAnErrorMessageWhenLoadingFails() async {
    let task = TestFactories.makeTask(title: "Write project plan")
    let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(result: .failure(WorkLogRepositoryStubError.fetchFailed)))

    await viewModel.loadEntries(for: task.id)

    #expect(viewModel.entries.isEmpty)
    #expect(viewModel.errorMessage == "Failed to load work logs.")
    #expect(viewModel.isLoading == false)
}

@MainActor
@Test func workLogListViewModelClearsEntriesWhenSubsequentLoadFails() async {
    let task = TestFactories.makeTask(title: "Write project plan")
    let entries = [TestFactories.makeWorkLogEntry(taskID: task.id)]
    let stub = MutableWorkLogRepositoryStub(result: .success(entries))
    let viewModel = WorkLogListViewModel(repository: stub)

    await viewModel.loadEntries(for: task.id)
    #expect(viewModel.entries == entries)

    stub.result = .failure(WorkLogRepositoryStubError.fetchFailed)
    await viewModel.loadEntries(for: task.id)

    #expect(viewModel.entries.isEmpty)
    #expect(viewModel.errorMessage == "Failed to load work logs.")
}

private struct WorkLogRepositoryStub: WorkLogRepository, Sendable {
    let result: Result<[WorkLogEntry], Error>

    func fetchEntries(for taskID: Task.ID) async throws -> [WorkLogEntry] {
        try result.get()
    }
}

private final class MutableWorkLogRepositoryStub: WorkLogRepository, @unchecked Sendable {
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
