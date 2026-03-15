import XCTest

@testable import App

final class TaskListViewModelTests: XCTestCase {
    @MainActor
    func testTaskListViewModelLoadsTasksFromRepository() async throws {
        let tasks = [
            try makeTask(title: "Write project plan", description: "Capture the current decisions."),
            try makeTask(title: "Review next step"),
        ]
        let viewModel = TaskListViewModel(repository: TaskRepositoryStub(result: .success(tasks)))

        await viewModel.loadTasks()

        XCTAssertEqual(viewModel.tasks, tasks)
        XCTAssertNil(viewModel.errorMessage)
        XCTAssertFalse(viewModel.isLoading)
    }

    @MainActor
    func testTaskListViewModelStoresAnErrorMessageWhenLoadingFails() async {
        let viewModel = TaskListViewModel(
            repository: TaskRepositoryStub(result: .failure(TaskRepositoryStubError.fetchFailed)))

        await viewModel.loadTasks()

        XCTAssertTrue(viewModel.tasks.isEmpty)
        XCTAssertEqual(viewModel.errorMessage, "Failed to load tasks.")
        XCTAssertFalse(viewModel.isLoading)
    }
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

private func makeTask(title: String, description: String = "") throws -> Task {
    try Task(title: title, description: description)
}
