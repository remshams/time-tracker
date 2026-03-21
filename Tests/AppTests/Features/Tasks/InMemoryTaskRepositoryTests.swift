import Testing

@testable import App

@Test func inMemoryTaskRepositoryReturnsSeededTasks() async throws {
    let tasks = [
        TestFactories.makeTask(title: "Write project plan", description: "Capture the current decisions."),
        TestFactories.makeTask(title: "Review next step"),
    ]
    let repository = InMemoryTaskRepository(tasks: tasks)

    let fetchedTasks = try await repository.fetchTasks()

    #expect(fetchedTasks == tasks)
}

@Test func inMemoryTaskRepositoryReturnsAnEmptyListByDefault() async throws {
    let repository = InMemoryTaskRepository()

    let fetchedTasks = try await repository.fetchTasks()

    #expect(fetchedTasks.isEmpty)
}
