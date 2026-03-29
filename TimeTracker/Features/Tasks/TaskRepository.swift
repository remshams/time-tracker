protocol TaskRepository: Sendable {
  func fetchTasks() async throws -> [WorkTask]
  func addTask(_ task: WorkTask) async throws
}
