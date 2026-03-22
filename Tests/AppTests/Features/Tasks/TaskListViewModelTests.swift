import Testing

@testable import App

@MainActor
@Test func taskListViewModelLoadsTasksFromRepository() async {
    let tasks = [
        TestFactories.makeTask(title: "Write project plan", description: "Capture the current decisions."),
        TestFactories.makeTask(title: "Review next step"),
    ]
    let viewModel = TaskListViewModel(repository: TaskRepositoryStub(result: .success(tasks)))

    await viewModel.loadTasks()

    #expect(viewModel.tasks == tasks)
    #expect(viewModel.errorMessage == nil)
    #expect(viewModel.isLoading == false)
}

@MainActor
@Test func taskListViewModelStoresAnErrorMessageWhenLoadingFails() async {
    let viewModel = TaskListViewModel(
        repository: TaskRepositoryStub(result: .failure(TaskRepositoryStubError.fetchFailed)))

    await viewModel.loadTasks()

    #expect(viewModel.tasks.isEmpty)
    #expect(viewModel.errorMessage == "Failed to load tasks.")
    #expect(viewModel.isLoading == false)
}

@MainActor
@Test func taskListViewModelIsNotLoadedBeforeFirstLoad() {
    let viewModel = TaskListViewModel(
        repository: TaskRepositoryStub(result: .success([])))

    #expect(viewModel.isLoaded == false)
}

@MainActor
@Test func taskListViewModelIsLoadedAfterSuccessfulLoad() async {
    let viewModel = TaskListViewModel(
        repository: TaskRepositoryStub(result: .success([])))

    await viewModel.loadTasks()

    #expect(viewModel.isLoaded == true)
}

@MainActor
@Test func taskListViewModelIsNotLoadedAfterFailedLoad() async {
    let viewModel = TaskListViewModel(
        repository: TaskRepositoryStub(result: .failure(TaskRepositoryStubError.fetchFailed)))

    await viewModel.loadTasks()

    #expect(viewModel.isLoaded == false)
}

private struct TaskRepositoryStub: TaskRepository, Sendable {
    let result: Result<[Task], Error>

    func fetchTasks() async throws -> [Task] {
        try result.get()
    }
}

private enum TaskRepositoryStubError: Error, Sendable {
    case fetchFailed
}
