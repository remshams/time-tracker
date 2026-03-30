actor InMemoryWorkTaskRepository: WorkTaskRepository {
  private var tasks: [WorkTask]

  init(tasks: [WorkTask] = []) {
    self.tasks = tasks
  }

  func fetchTasks() async throws -> [WorkTask] {
    tasks
  }

  func addTask(_ task: WorkTask) async throws {
    tasks.append(task)
  }
}
