import Foundation
import Observation

public enum TaskArchivingAction: Equatable, Sendable {
    case archive
    case unarchive
}

/// Holds the task archive dialog and sends archive and unarchive requests.
///
/// The sheet, the context menus and the toolbar read this store from the SwiftUI environment. Unarchiving sends its
/// request without showing the dialog, and only a failure presents it. Closing leaves the content in place, because
/// SwiftUI renders the sheet during its dismissal animation. The next action replaces it.
@Observable
public final class TaskArchivingStore {
    public private(set) var isPresented = false
    public private(set) var taskName = ""
    public private(set) var action = TaskArchivingAction.archive
    public private(set) var isSubmitting = false
    public private(set) var error: String?
    private(set) var taskID = ""

    private let client: any TrackerClient
    private let clock: any TrackerClock
    private let connection: ConnectionStore
    private let catalog: TaskCatalogStore
    private let tracking: TrackingStore
    private let reloadAfterWrite: @MainActor (_ then: () -> Void) async -> Void

    init(
        client: any TrackerClient, clock: any TrackerClock, connection: ConnectionStore, catalog: TaskCatalogStore,
        tracking: TrackingStore, reloadAfterWrite: @escaping @MainActor (_ then: () -> Void) async -> Void
    ) {
        self.client = client
        self.clock = clock
        self.connection = connection
        self.catalog = catalog
        self.tracking = tracking
        self.reloadAfterWrite = reloadAfterWrite
    }

    public var canSubmit: Bool { isPresented && !isSubmitting }

    public func canArchive(taskID: String) -> Bool {
        canBegin && tracking.active?.taskId != taskID && task(withID: taskID)?.archived == false
    }

    public func canUnarchive(taskID: String) -> Bool {
        canBegin && task(withID: taskID)?.archived == true
    }

    public func openArchive(taskID: String) {
        guard canArchive(taskID: taskID), let task = task(withID: taskID) else { return }
        capture(task, action: .archive)
        isPresented = true
    }

    public func unarchive(taskID: String) {
        guard canUnarchive(taskID: taskID), let task = task(withID: taskID) else { return }
        capture(task, action: .unarchive)
        send()
    }

    public func cancel() {
        guard !isSubmitting else { return }
        isPresented = false
    }

    public func submit() {
        guard canSubmit else { return }
        send()
    }

    private var canBegin: Bool { connection.confirmed && !isPresented && !isSubmitting }

    private func task(withID id: String) -> TaskItem? { catalog.tasks.first { $0.id == id } }

    private func capture(_ task: TaskItem, action: TaskArchivingAction) {
        taskID = task.id
        taskName = task.name
        self.action = action
        error = nil
    }

    private func send() {
        isSubmitting = true
        error = nil
        let occurredAt = formatTimestamp(clock.now)
        // The dialog can open for another task during the reload, so the reload's follow-up reads these copies.
        let taskID = self.taskID
        let taskName = self.taskName
        let action = self.action
        Task { [weak self] in
            guard let self else { return }
            do {
                switch action {
                case .archive:
                    _ = try await client.archiveTask(taskID: taskID, expectedName: taskName, occurredAt: occurredAt)
                case .unarchive:
                    _ = try await client.unarchiveTask(taskID: taskID, occurredAt: occurredAt)
                }
                isPresented = false
                isSubmitting = false
                await reloadAfterWrite {
                    if action == .unarchive { self.catalog.rememberRestoredTask(taskID) }
                }
            } catch {
                // An unarchive starts without the dialog, so its failure presents it.
                isPresented = true
                isSubmitting = false
                self.error = writeFailure(error).localizedDescription
                await reloadAfterWrite({})
            }
        }
    }
}
