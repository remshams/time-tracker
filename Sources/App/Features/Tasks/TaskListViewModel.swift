import Foundation

@Observable
@MainActor
final class TaskListViewModel {
  private(set) var tasks: [Task] = []
  private(set) var loadingState: LoadingState = .idle

  var isLoading: Bool { loadingState.isLoading }
  var isLoaded: Bool { loadingState.isLoaded }
  var errorMessage: String? { loadingState.errorMessage }

  private let repository: any TaskRepository

  init(repository: any TaskRepository) {
    self.repository = repository
  }

  func loadTasks() async {
    tasks = []
    loadingState = .loading

    do {
      tasks = try await repository.fetchTasks()
      loadingState = .loaded
    } catch {
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
