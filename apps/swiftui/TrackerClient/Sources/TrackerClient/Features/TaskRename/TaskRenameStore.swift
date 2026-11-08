import Foundation
import Observation

/// Holds the task rename dialog and sends its request.
///
/// The sheet, the context menus and the toolbar read this store from the SwiftUI environment. Closing leaves the content in
/// place, because SwiftUI renders the sheet during its dismissal animation. `open` replaces it.
@Observable
public final class TaskRenameStore {
    public private(set) var isPresented = false
    public private(set) var originalName = ""
    public private(set) var name = ""
    public private(set) var isSubmitting = false
    public private(set) var error: String?
    private(set) var taskID = ""

    private let client: any TrackerClient
    private let clock: any TrackerClock
    private let connection: ConnectionStore
    private let catalog: TaskCatalogStore
    private let reloadAfterWrite: @MainActor (_ then: () -> Void) async -> Void

    init(
        client: any TrackerClient, clock: any TrackerClock, connection: ConnectionStore, catalog: TaskCatalogStore,
        reloadAfterWrite: @escaping @MainActor (_ then: () -> Void) async -> Void
    ) {
        self.client = client
        self.clock = clock
        self.connection = connection
        self.catalog = catalog
        self.reloadAfterWrite = reloadAfterWrite
    }

    public var canOpen: Bool { connection.confirmed && catalog.selectedTask != nil }

    public var canSubmit: Bool {
        let desired = TaskNameEditingPolicy.normalized(name)
        return isPresented && !isSubmitting && !desired.isEmpty && desired != originalName
    }

    public func open(taskID: String) {
        guard canOpen, !isPresented, let task = catalog.tasks.first(where: { $0.id == taskID }) else { return }
        self.taskID = task.id
        originalName = task.name
        name = task.name
        error = nil
        isPresented = true
    }

    public func setName(_ value: String) {
        guard isPresented, !isSubmitting, value != name else { return }
        name = value
        error = nil
    }

    public func cancel() {
        guard !isSubmitting else { return }
        isPresented = false
    }

    public func submit() {
        guard canSubmit else { return }
        if let message = TaskNameEditingPolicy.transportError(name) {
            error = message
            return
        }
        isSubmitting = true
        error = nil
        let occurredAt = formatTimestamp(clock.now)
        Task { [weak self] in
            guard let self else { return }
            do {
                _ = try await client.renameTask(
                    taskID: taskID, expectedName: originalName, name: name, occurredAt: occurredAt)
                isPresented = false
            } catch {
                self.error = writeFailure(error).localizedDescription
            }
            isSubmitting = false
            await reloadAfterWrite({})
        }
    }
}
