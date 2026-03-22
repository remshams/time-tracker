import Testing

@testable import App

@MainActor
@Test func workLogListViewModelLoadsEntriesFromRepository() async {
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

@MainActor
@Test func workLogListViewModelExposesEmptyEntriesBeforeLoad() {
    let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(result: .success([])))

    #expect(viewModel.entries.isEmpty)
}

@MainActor
@Test func workLogListViewModelStoresAnErrorMessageWhenLoadingFails() async {
    let task = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
    let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(result: .failure(WorkLogRepositoryStubError.fetchFailed)))

    await viewModel.loadEntries(for: task.id)

    #expect(viewModel.entries.isEmpty)
    #expect(viewModel.errorMessage == "Failed to load work logs.")
    #expect(viewModel.isLoading == false)
}

@MainActor
@Test func workLogListViewModelClearsEntriesWhenSubsequentLoadFails() async {
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

@MainActor
@Test func workLogListViewModelIsNotLoadedBeforeFirstLoad() {
    let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(result: .success([])))

    #expect(viewModel.isLoaded == false)
}

@MainActor
@Test func workLogListViewModelIsLoadedAfterSuccessfulLoad() async {
    let task = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
    let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(result: .success([])))

    await viewModel.loadEntries(for: task.id)

    #expect(viewModel.isLoaded == true)
}

@MainActor
@Test func workLogListViewModelIsNotLoadedAfterFailedLoad() async {
    let task = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
    let viewModel = WorkLogListViewModel(
        repository: WorkLogRepositoryStub(result: .failure(WorkLogRepositoryStubError.fetchFailed)))

    await viewModel.loadEntries(for: task.id)

    #expect(viewModel.isLoaded == false)
}

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
