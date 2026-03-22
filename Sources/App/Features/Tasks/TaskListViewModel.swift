import Foundation

@MainActor
final class TaskListViewModel: ObservableObject {
    @Published private(set) var tasks: [Task] = []
    @Published private(set) var loadingState: LoadingState = .idle

    var isLoading: Bool { loadingState.isLoading }
    var isLoaded: Bool { loadingState.isLoaded }
    var errorMessage: String? { loadingState.errorMessage }

    private let repository: any TaskRepository

    init(repository: any TaskRepository) {
        self.repository = repository
    }

    func loadTasks() async {
        loadingState = .loading

        do {
            tasks = try await repository.fetchTasks()
            loadingState = .loaded
        } catch {
            tasks = []
            loadingState = .failed(
                String(
                    localized: "task-list.error.message",
                    defaultValue: "Failed to load tasks."))
        }
    }

    func task(for id: Task.ID) -> Task? {
        tasks.first { $0.id == id }
    }
}
