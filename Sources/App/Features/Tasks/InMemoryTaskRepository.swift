struct InMemoryTaskRepository: TaskRepository, Sendable {
  private let tasks: [Task]

  init(tasks: [Task] = []) {
    self.tasks = tasks
  }

  func fetchTasks() async throws -> [Task] {
    tasks
  }
}
