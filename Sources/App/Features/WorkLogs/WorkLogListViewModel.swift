import Foundation

@MainActor
final class WorkLogListViewModel: ObservableObject {
    @Published private(set) var entries: [WorkLogEntry] = []
    @Published private(set) var isLoading = false
    @Published private(set) var errorMessage: String?

    private let repository: any WorkLogRepository

    init(repository: any WorkLogRepository) {
        self.repository = repository
    }

    func loadEntries(for taskID: Task.ID) async {
        isLoading = true
        errorMessage = nil

        defer {
            isLoading = false
        }

        do {
            entries = try await repository.fetchEntries(for: taskID)
        } catch {
            entries = []
            errorMessage = String(
                localized: "work-log-list.error.message",
                defaultValue: "Failed to load work logs.")
        }
    }
}
