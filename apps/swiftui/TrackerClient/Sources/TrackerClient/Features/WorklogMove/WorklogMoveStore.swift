import Foundation
import Observation

/// Holds the worklog move dialog, searches its destination tasks and sends its request.
///
/// The sheet and the worklog menus read this store from the SwiftUI environment. Closing leaves the content in place,
/// because SwiftUI renders the sheet during its dismissal animation. `open` replaces it.
@Observable
public final class WorklogMoveStore {
    public private(set) var isPresented = false
    public private(set) var original: WorklogItem?
    public private(set) var sourceTaskName = ""
    public private(set) var query = ""
    public private(set) var candidates: [WorklogMoveCandidate] = []
    public private(set) var selectedTaskID: String?
    public private(set) var isSearching = false
    public private(set) var isSubmitting = false
    public private(set) var error: String?
    private var revision = 0

    private let client: any TrackerClient
    private let connection: ConnectionStore
    private let catalog: TaskCatalogStore
    private let history: WorklogHistoryStore
    private let tracking: TrackingStore
    private let reloadAfterWrite: @MainActor () async -> Void

    init(
        client: any TrackerClient, connection: ConnectionStore, catalog: TaskCatalogStore,
        history: WorklogHistoryStore, tracking: TrackingStore,
        reloadAfterWrite: @escaping @MainActor () async -> Void
    ) {
        self.client = client
        self.connection = connection
        self.catalog = catalog
        self.history = history
        self.tracking = tracking
        self.reloadAfterWrite = reloadAfterWrite
    }

    public var canOpen: Bool { connection.confirmed }
    public var canEdit: Bool { isPresented && !isSubmitting }
    public var canSubmit: Bool { canEdit && !isSearching && selectedTaskID != nil }

    public func open(worklogID: String) {
        guard canOpen, !isPresented, let worklog = worklog(withID: worklogID) else { return }
        original = worklog
        sourceTaskName = catalog.tasks.first { $0.id == worklog.taskId }?.name ?? "Unknown task"
        query = ""
        candidates = []
        isPresented = true
        search()
    }

    public func setQuery(_ value: String) {
        guard canEdit, value != query else { return }
        query = value
        search()
    }

    public func retryCandidates() {
        guard canEdit else { return }
        search()
    }

    public func select(taskID: String) {
        guard canEdit, !isSearching, candidates.contains(where: { $0.id == taskID }) else { return }
        selectedTaskID = taskID
        error = nil
    }

    public func moveSelection(by offset: Int) {
        guard canEdit, !isSearching, !candidates.isEmpty else { return }
        let index = candidates.firstIndex { $0.id == selectedTaskID } ?? 0
        selectedTaskID = candidates[min(max(index + offset, 0), candidates.count - 1)].id
    }

    public func cancel() {
        guard !isSubmitting else { return }
        isPresented = false
    }

    public func submit() {
        guard canSubmit, let original, let selectedTaskID else { return }
        isSubmitting = true
        error = nil
        Task { [weak self] in
            guard let self else { return }
            do {
                _ = try await client.moveWorklog(expected: original, destinationTaskID: selectedTaskID)
                isPresented = false
            } catch {
                self.error = writeFailure(error).localizedDescription
            }
            isSubmitting = false
            await reloadAfterWrite()
        }
    }

    /// Starts a new search revision. Results for older revisions are dropped.
    private func search() {
        guard let sourceTaskID = original?.taskId else { return }
        revision += 1
        selectedTaskID = nil
        error = nil
        isSearching = true
        Task { [weak self, revision, query] in
            guard let self else { return }
            do {
                let values = try await client.moveCandidates(sourceTaskID: sourceTaskID, query: query)
                acceptCandidates(values, revision: revision)
            } catch {
                failSearch(error, revision: revision)
            }
        }
    }

    private func acceptCandidates(_ values: [WorklogMoveCandidate], revision: Int) {
        guard isPresented, revision == self.revision else { return }
        candidates = values.filter { $0.id != original?.taskId }
        selectedTaskID = candidates.first?.id
        isSearching = false
    }

    private func failSearch(_ failure: Error, revision: Int) {
        guard isPresented, revision == self.revision else { return }
        candidates = []
        isSearching = false
        error = failure.localizedDescription
    }

    /// A worklog from the shown history, or the running one, which the history may not have loaded.
    private func worklog(withID id: String) -> WorklogItem? {
        history.worklogs.first { $0.id == id } ?? (tracking.active?.id == id ? tracking.active : nil)
    }
}
