import Testing

@testable import App

@Test func taskListViewModelLoadsTasksFromRepository() async {
    let tasks = [
        makeTask(title: "Write project plan", description: "Capture the current decisions."),
        makeTask(title: "Review next step"),
    ]
    let viewModel = await MainActor.run {
        TaskListViewModel(repository: TaskRepositoryStub(result: .success(tasks)))
    }

    await viewModel.loadTasks()
    let (loadedTasks, errorMessage, isLoading) = await MainActor.run {
        (viewModel.tasks, viewModel.errorMessage, viewModel.isLoading)
    }

    #expect(loadedTasks == tasks)
    #expect(errorMessage == nil)
    #expect(isLoading == false)
}

@Test func taskListViewModelStoresAnErrorMessageWhenLoadingFails() async {
    let viewModel = await MainActor.run {
        TaskListViewModel(
            repository: TaskRepositoryStub(result: .failure(TaskRepositoryStubError.fetchFailed)))
    }

    await viewModel.loadTasks()
    let (tasks, errorMessage, isLoading) = await MainActor.run {
        (viewModel.tasks, viewModel.errorMessage, viewModel.isLoading)
    }

    #expect(tasks.isEmpty)
    #expect(errorMessage == "Failed to load tasks.")
    #expect(isLoading == false)
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

private func makeTask(title: String, description: String = "") -> Task {
    try! Task(title: title, description: description)
}
