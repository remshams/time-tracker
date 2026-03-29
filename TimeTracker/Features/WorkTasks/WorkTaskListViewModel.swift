import Foundation

@Observable
@MainActor
final class WorkTaskListViewModel {
  private(set) var tasks: [WorkTask] = []
  private(set) var loadingState: LoadingState = .idle

  var isLoading: Bool { loadingState.isLoading }
  var isLoaded: Bool { loadingState.isLoaded }
  var errorMessage: String? { loadingState.errorMessage }

  private let repository: any WorkTaskRepository

  init(repository: any WorkTaskRepository) {
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

  func task(for id: WorkTask.ID) -> WorkTask? {
    tasks.first { $0.id == id }
  }

  func createTask(title: String, description: String) async {
    do {
      let task = try WorkTask(title: title, description: description.isEmpty ? nil : description)
      try await repository.addTask(task)
      await loadTasks()
    } catch {
      loadingState = .failed(
        String(
          localized: "task-list.create-error.message",
          defaultValue: "Failed to create task."))
    }
  }
}
