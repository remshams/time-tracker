actor InMemoryTaskRepository: TaskRepository {
  private var tasks: [Task]

  init(tasks: [Task] = []) {
    self.tasks = tasks
  }

  func fetchTasks() async throws -> [Task] {
    tasks
  }

  func addTask(_ task: Task) async throws {
    tasks.append(task)
  }
}
