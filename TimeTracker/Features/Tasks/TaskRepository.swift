protocol TaskRepository: Sendable {
  func fetchTasks() async throws -> [Task]
  func addTask(_ task: Task) async throws
}
