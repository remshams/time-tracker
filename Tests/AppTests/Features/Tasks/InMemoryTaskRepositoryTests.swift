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

@Test func inMemoryTaskRepositoryAddedTaskAppearsInFetchTasks() async throws {
  let repository = InMemoryTaskRepository()
  let task = TestFactories.makeTask(title: "New task")

  try await repository.addTask(task)
  let fetchedTasks = try await repository.fetchTasks()

  #expect(fetchedTasks == [task])
}

@Test func inMemoryTaskRepositoryMultipleAddedTasksAllAppearInFetchTasks() async throws {
  let repository = InMemoryTaskRepository()
  let firstTask = TestFactories.makeTask(title: "First task")
  let secondTask = TestFactories.makeTask(title: "Second task")

  try await repository.addTask(firstTask)
  try await repository.addTask(secondTask)
  let fetchedTasks = try await repository.fetchTasks()

  #expect(fetchedTasks == [firstTask, secondTask])
}

@Test func inMemoryTaskRepositorySeededTasksArePreservedAfterAdd() async throws {
  let seededTask = TestFactories.makeTask(title: "Seeded task")
  let repository = InMemoryTaskRepository(tasks: [seededTask])
  let newTask = TestFactories.makeTask(title: "New task")

  try await repository.addTask(newTask)
  let fetchedTasks = try await repository.fetchTasks()

  #expect(fetchedTasks == [seededTask, newTask])
}
