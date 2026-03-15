import XCTest

@testable import App

final class AppTests: XCTestCase {
    @MainActor
    func testContentViewModuleLoads() throws {
        let repository = InMemoryTaskRepository(tasks: [
            try makeTask(title: "Write project plan", description: "Capture the current decisions."),
            try makeTask(title: "Review next step"),
        ])

        let view = ContentView(viewModel: TaskListViewModel(repository: repository))

        XCTAssertEqual(String(describing: type(of: view)), "ContentView")
    }

    @MainActor
    func testTaskListViewModuleLoads() throws {
        let repository = InMemoryTaskRepository(tasks: [
            try makeTask(title: "Write project plan", description: "Capture the current decisions."),
            try makeTask(title: "Review next step"),
        ])

        let view = TaskListView(viewModel: TaskListViewModel(repository: repository))

        XCTAssertEqual(String(describing: type(of: view)), "TaskListView")
    }
}

private func makeTask(title: String, description: String? = nil) throws -> Task {
    try Task(title: title, description: description)
}
