import Foundation
import Observation

/// Holds the worklog deletion dialog and sends its request.
///
/// The sheet and the worklog menus read this store from the SwiftUI environment. Closing leaves the content in place,
/// because SwiftUI renders the sheet during its dismissal animation. `open` replaces it.
@Observable
public final class WorklogDeletionStore {
    public private(set) var isPresented = false {
        didSet { gate.update(self, isShown: isPresented) }
    }
    public private(set) var taskName = ""
    public private(set) var original: WorklogItem?
    public private(set) var isSubmitting = false
    public private(set) var error: String?

    private let client: any TrackerClient
    private let connection: ConnectionStore
    private let catalog: TaskCatalogStore
    private let history: WorklogHistoryStore
    private let gate: PresentationGate
    private let reloadAfterWrite: @MainActor () async -> Void

    init(
        client: any TrackerClient, connection: ConnectionStore, catalog: TaskCatalogStore,
        history: WorklogHistoryStore, gate: PresentationGate,
        reloadAfterWrite: @escaping @MainActor () async -> Void
    ) {
        self.client = client
        self.connection = connection
        self.catalog = catalog
        self.history = history
        self.gate = gate
        self.reloadAfterWrite = reloadAfterWrite
    }

    public var canOpen: Bool { connection.confirmed && !gate.isClaimed }

    public var canSubmit: Bool { isPresented && !isSubmitting }

    public func open(worklogID: String) {
        // Running worklogs cannot be deleted. Stop the timer first.
        guard canOpen, let worklog = history.worklogs.first(where: { $0.id == worklogID }),
            worklog.end != nil
        else { return }
        original = worklog
        taskName = catalog.tasks.first(where: { $0.id == worklog.taskId })?.name ?? "Unknown task"
        error = nil
        isPresented = true
    }

    public func cancel() {
        guard !isSubmitting else { return }
        isPresented = false
    }

    public func submit() {
        guard canSubmit, let original else { return }
        isSubmitting = true
        error = nil
        Task { [weak self] in
            guard let self else { return }
            do {
                _ = try await client.deleteWorklog(expected: original)
                isPresented = false
            } catch {
                self.error = writeFailure(error).localizedDescription
            }
            isSubmitting = false
            await reloadAfterWrite()
        }
    }
}
