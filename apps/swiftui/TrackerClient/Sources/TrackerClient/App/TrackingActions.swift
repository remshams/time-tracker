import Foundation

/// Starts and stops tracking from the window and the menu bar, and decides which of those requests are
/// possible right now.
///
/// `TrackerSession` creates it with the session's stores and `TrackerSync`. After each request it asks the sync
/// to cancel a pending auto-resume, reload the task list, record a failure and publish. The sync never calls
/// back into this class.
public final class TrackingActions {
    private let client: any TrackerClient
    private let clock: any TrackerClock
    private let sync: TrackerSync
    private let catalog: TaskCatalogStore
    private let connection: ConnectionStore
    private let tracking: TrackingStore
    private let menuBar: MenuBarStore
    private let automation: TrackingAutomationStore

    init(
        client: any TrackerClient, clock: any TrackerClock, sync: TrackerSync, catalog: TaskCatalogStore,
        connection: ConnectionStore, tracking: TrackingStore, menuBar: MenuBarStore,
        automation: TrackingAutomationStore
    ) {
        self.client = client
        self.clock = clock
        self.sync = sync
        self.catalog = catalog
        self.connection = connection
        self.tracking = tracking
        self.menuBar = menuBar
        self.automation = automation
    }

    func canStartTracking(taskID: String) -> Bool {
        guard canSendTracking, let task = catalog.tasks.first(where: { $0.id == taskID }) else { return false }
        return !task.archived && tracking.active?.taskId != task.id
    }

    var canStopTracking: Bool { canSendTracking && tracking.active != nil }

    private var canSendTracking: Bool { connection.confirmed && !connection.stale && !tracking.isSubmitting }

    var menuPrimaryAction: MenuPrimaryAction {
        guard !automation.locked, canSendTracking else { return .disabled }
        if let active = tracking.active { return .stop(worklogID: active.id) }
        let lastTrackedTask = menuBar.lastTrackedTaskID.flatMap { id in catalog.tasks.first { $0.id == id } }
        guard let task = lastTrackedTask, !task.archived else { return .openMenu }
        return .start(taskID: task.id)
    }

    @discardableResult
    public func performMenuPrimaryAction() -> MenuPrimaryAction {
        let action = menuPrimaryAction
        switch action {
        case .stop(let worklogID): stopTracking(worklogID: worklogID)
        case .start(let taskID): startTracking(taskID: taskID)
        case .openMenu, .disabled: break
        }
        return action
    }

    public func startTracking(taskID: String) {
        guard canStartTracking(taskID: taskID) else { return }
        let expectedActiveID = tracking.active?.id
        let occurredAt = formatTimestamp(clock.now)
        sendTracking { client in
            _ = try await client.startTracking(
                taskID: taskID, expectedActiveID: expectedActiveID, occurredAt: occurredAt)
        }
    }

    public func stopTracking(worklogID: String) {
        guard canStopTracking, tracking.active?.id == worklogID else { return }
        let occurredAt = formatTimestamp(clock.now)
        sendTracking { client in
            _ = try await client.stopTracking(worklogID: worklogID, occurredAt: occurredAt)
        }
    }

    public func dismissTrackingError() {
        tracking.clearError()
        sync.publish()
    }

    private func sendTracking(_ request: @escaping @MainActor (any TrackerClient) async throws -> Void) {
        sync.cancelAutoResume()
        tracking.clearError()
        tracking.beginSubmitting()
        sync.publish()
        Task { [weak self] in
            guard let self else { return }
            do {
                try await request(client)
            } catch {
                sync.recordTrackingFailure(error)
            }
            await sync.reloadTaskList()
            tracking.endSubmitting()
            sync.publish()
        }
    }
}
