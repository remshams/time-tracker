import Foundation

@MainActor
final class WorkLogListViewModel: ObservableObject {
    @Published private(set) var entries: [WorkLogEntry] = []
    @Published private(set) var loadingState: LoadingState = .idle

    var isLoading: Bool { loadingState.isLoading }
    var isLoaded: Bool { loadingState.isLoaded }
    var errorMessage: String? { loadingState.errorMessage }

    private let repository: any WorkLogRepository

    init(repository: any WorkLogRepository) {
        self.repository = repository
    }

    func loadEntries(for taskID: Task.ID) async {
        entries = []
        loadingState = .loading

        do {
            entries = try await repository.fetchEntries(for: taskID)
            loadingState = .loaded
        } catch {
            loadingState = .failed(
                String(
                    localized: "work-log-list.error.message",
                    defaultValue: "Failed to load work logs."))
        }
    }
}
