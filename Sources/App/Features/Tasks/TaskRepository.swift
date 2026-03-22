protocol TaskRepository: Sendable {
  func fetchTasks() async throws -> [Task]
}
