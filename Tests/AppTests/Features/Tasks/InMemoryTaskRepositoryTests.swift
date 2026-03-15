import XCTest

@testable import App

final class InMemoryTaskRepositoryTests: XCTestCase {
    func testInMemoryTaskRepositoryReturnsSeededTasks() async throws {
        let tasks = [
            try Task(title: "Write project plan", description: "Capture the current decisions."),
            try Task(title: "Review next step"),
        ]
        let repository = InMemoryTaskRepository(tasks: tasks)

        let fetchedTasks = try await repository.fetchTasks()

        XCTAssertEqual(fetchedTasks, tasks)
    }

    func testInMemoryTaskRepositoryReturnsAnEmptyListByDefault() async throws {
        let repository = InMemoryTaskRepository()

        let fetchedTasks = try await repository.fetchTasks()

        XCTAssertTrue(fetchedTasks.isEmpty)
    }
}
