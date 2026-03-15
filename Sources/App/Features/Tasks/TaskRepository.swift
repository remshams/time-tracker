protocol TaskRepository {
    func fetchTasks() async throws -> [Task]
}
