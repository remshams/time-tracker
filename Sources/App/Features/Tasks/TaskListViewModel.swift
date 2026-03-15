import Combine
import Foundation

@MainActor
final class TaskListViewModel: ObservableObject {
    @Published private(set) var tasks: [Task] = []
    @Published private(set) var isLoading = false
    @Published private(set) var errorMessage: String?

    private let repository: any TaskRepository

    init(repository: any TaskRepository) {
        self.repository = repository
    }

    func loadTasks() async {
        isLoading = true
        errorMessage = nil

        defer {
            isLoading = false
        }

        do {
            tasks = try await repository.fetchTasks()
        } catch {
            tasks = []
            errorMessage = "Failed to load tasks."
        }
    }
}
