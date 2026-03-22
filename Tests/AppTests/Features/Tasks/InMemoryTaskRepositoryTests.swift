import Testing

@testable import App

@Suite struct InMemoryTaskRepositoryTests {
  @Suite struct FetchTasks {
    @Test func returnsSeededTasks() async throws {
      let tasks = [
        TestFactories.makeTask(title: "Write project plan", description: "Capture the current decisions."),
        TestFactories.makeTask(title: "Review next step"),
      ]
      let repository = InMemoryTaskRepository(tasks: tasks)

      let fetchedTasks = try await repository.fetchTasks()

      #expect(fetchedTasks == tasks)
    }

    @Test func returnsAnEmptyListByDefault() async throws {
      let repository = InMemoryTaskRepository()

      let fetchedTasks = try await repository.fetchTasks()

      #expect(fetchedTasks.isEmpty)
    }
  }

  @Suite struct AddTask {
    private static let newTaskTitle = "New task"
    private static let seededTaskTitle = "Seeded task"
    private static let firstTaskTitle = "First task"
    private static let secondTaskTitle = "Second task"

    private let repository = InMemoryTaskRepository()

    @Test func addedTaskAppearsInFetchTasks() async throws {
      let task = TestFactories.makeTask(title: Self.newTaskTitle)

      try await repository.addTask(task)
      let fetchedTasks = try await repository.fetchTasks()

      #expect(fetchedTasks == [task])
    }

    @Test func multipleAddedTasksAllAppearInFetchTasks() async throws {
      let firstTask = TestFactories.makeTask(title: Self.firstTaskTitle)
      let secondTask = TestFactories.makeTask(title: Self.secondTaskTitle)

      try await repository.addTask(firstTask)
      try await repository.addTask(secondTask)
      let fetchedTasks = try await repository.fetchTasks()

      #expect(fetchedTasks == [firstTask, secondTask])
    }

    @Test func seededTasksArePreservedAfterAdd() async throws {
      let seededTask = TestFactories.makeTask(title: Self.seededTaskTitle)
      let seededRepository = InMemoryTaskRepository(tasks: [seededTask])
      let newTask = TestFactories.makeTask(title: Self.newTaskTitle)

      try await seededRepository.addTask(newTask)
      let fetchedTasks = try await seededRepository.fetchTasks()

      #expect(fetchedTasks == [seededTask, newTask])
    }

    @Test func handlesConcurrentAdds() async throws {
      let firstTask = TestFactories.makeTask(title: Self.firstTaskTitle)
      let secondTask = TestFactories.makeTask(title: Self.secondTaskTitle)

      try await withThrowingTaskGroup(of: Void.self) { group in
        group.addTask { try await repository.addTask(firstTask) }
        group.addTask { try await repository.addTask(secondTask) }
        try await group.waitForAll()
      }

      let fetched = try await repository.fetchTasks()
      #expect(fetched.count == 2)
      #expect(fetched.contains(firstTask))
      #expect(fetched.contains(secondTask))
    }
  }
}
