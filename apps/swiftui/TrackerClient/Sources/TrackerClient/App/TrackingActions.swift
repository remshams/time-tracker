import Foundation
import Observation

/// Starts and stops tracking from the window and the menu bar. `TrackingAvailability` decides which of those
/// requests are possible right now.
///
/// `TrackerFeatures` creates it with the stores and `TrackerSync`. After each request it asks the sync
/// to cancel a pending auto-resume, reload the task list, record a failure and publish. The sync never calls
/// back into this class.
///
/// It is observable only so that views can read it from the SwiftUI environment. It stores only constants, so views
/// observe nothing on it.
@Observable
public final class TrackingActions {
    private let client: any TrackerClient
    private let clock: any TrackerClock
    private let sync: TrackerSync
    private let catalog: TaskCatalogStore
    private let availability: TrackingAvailability
    private let tracking: TrackingStore
    private let menuBar: MenuBarStore
    private let automation: TrackingAutomationStore

    init(
        client: any TrackerClient, clock: any TrackerClock, sync: TrackerSync, catalog: TaskCatalogStore,
        availability: TrackingAvailability, tracking: TrackingStore, menuBar: MenuBarStore,
        automation: TrackingAutomationStore
    ) {
        self.client = client
        self.clock = clock
        self.sync = sync
        self.catalog = catalog
        self.availability = availability
        self.tracking = tracking
        self.menuBar = menuBar
        self.automation = automation
    }

    var menuPrimaryAction: MenuPrimaryAction {
        guard !automation.locked, availability.canSend else { return .disabled }
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
        guard availability.canStart(taskID: taskID) else { return }
        let expectedActiveID = tracking.active?.id
        let occurredAt = formatTimestamp(clock.now)
        sendTracking { client in
            _ = try await client.startTracking(
                taskID: taskID, expectedActiveID: expectedActiveID, occurredAt: occurredAt)
        }
    }

    public func stopTracking(worklogID: String) {
        guard availability.canStop, tracking.active?.id == worklogID else { return }
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
            // The reload publishes on every path, which also shows a failure recorded above.
            await sync.reloadTaskList()
            tracking.endSubmitting()
        }
    }
}
