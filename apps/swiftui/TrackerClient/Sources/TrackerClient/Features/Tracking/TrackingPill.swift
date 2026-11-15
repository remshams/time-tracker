import Foundation
import Observation

/// What the tracking pill shows for one task: the running worklog, or the selected task it would start.
public struct TrackingPillState: Equatable, Sendable {
    public enum Mode: Equatable, Sendable {
        case running(worklogID: String, elapsedSeconds: Int?)
        case idle
    }

    public let mode: Mode
    public let taskID: String
    public let name: String

    public var isRunning: Bool { mode != .idle }

    /// The elapsed time of the running worklog, or `nil` while idle or before the tracker state is confirmed.
    public var elapsed: DurationText? {
        guard case .running(_, let seconds?) = mode else { return nil }
        return DurationText(seconds: seconds)
    }

    /// The button's accessibility label. The native E2E tests look the button up by it.
    public var actionLabel: String { isRunning ? "Stop tracking" : "Start tracking" }

    /// The tooltip, which names the action and the task.
    public func help(stale: Bool) -> String {
        let action = "\(actionLabel) \(name)"
        return stale ? "\(action). The connection is stale." : action
    }

    /// The task name, the spoken duration while running, and the stale note.
    public func accessibilityValue(stale: Bool) -> String {
        var parts = [name]
        if case .running(_, let seconds?) = mode { parts.append(spokenDuration(seconds)) }
        if stale { parts.append("connection stale") }
        return parts.joined(separator: ", ")
    }
}

/// The toolbar's one start and stop control. It shows the running task, or the selected task while nothing runs, and
/// one click stops or starts it.
///
/// While its own request is in flight the pill keeps showing the state from the moment of the click, so the reload
/// doesn't flicker through intermediate states. After a stop from the pill succeeds, it announces the stop.
@Observable
public final class TrackingPill {
    /// The state from the moment of the click, shown while the request it sent is in flight.
    private var frozen: TrackingPillState?
    /// Posts a VoiceOver announcement. The app sets it; tests record the calls.
    @ObservationIgnored public var announce: @MainActor (String) -> Void = { _ in }
    private let tracking: TrackingStore
    private let catalog: TaskCatalogStore
    private let connection: ConnectionStore
    private let availability: TrackingAvailability

    init(
        tracking: TrackingStore, catalog: TaskCatalogStore, connection: ConnectionStore,
        availability: TrackingAvailability
    ) {
        self.tracking = tracking
        self.catalog = catalog
        self.connection = connection
        self.availability = availability
    }

    /// The state to show, or `nil` for "Nothing running": no task runs and no active task is selected.
    public var shown: TrackingPillState? {
        if tracking.isSubmitting, let frozen { return frozen }
        return current
    }

    /// The task of the shown state. Unlike `shown`, it doesn't change with the elapsed time, so a sidebar row can read
    /// it without redrawing every second.
    public var taskID: String? {
        if tracking.isSubmitting, let frozen { return frozen.taskID }
        return tracking.active?.taskId ?? idleTask?.id
    }

    /// Whether a click can send a request now. A stale connection or a request in flight disables the pill.
    public var isEnabled: Bool {
        guard let shown else { return false }
        return shown.isRunning ? availability.canStop : availability.canStart(taskID: shown.taskID)
    }

    /// Whether a tracking request is in flight. The view keeps its look while it waits.
    public var isInFlight: Bool { tracking.isSubmitting }

    public var isStale: Bool { connection.stale }

    /// Stops the running task, or starts the selected one while nothing runs.
    public func click() {
        // `isEnabled` makes the same checks as the store, so the store sends the request and calls back.
        guard isEnabled, let pill = current else { return }
        frozen = pill
        switch pill.mode {
        case .running(let worklogID, _):
            tracking.stopTracking(worklogID: worklogID) { [weak self] in self?.finishStop(of: pill.name) }
        case .idle:
            tracking.startTracking(taskID: pill.taskID) { [weak self] in self?.frozen = nil }
        }
    }

    /// Selects the shown task and changes to its tab.
    public func showTask() {
        guard let shown, let task = catalog.tasks.first(where: { $0.id == shown.taskID }) else { return }
        catalog.showTask(task)
    }

    private var current: TrackingPillState? {
        if let active = tracking.active {
            let name = catalog.tasks.first { $0.id == active.taskId }?.name ?? "Unknown task"
            return TrackingPillState(
                mode: .running(worklogID: active.id, elapsedSeconds: tracking.elapsedSeconds), taskID: active.taskId,
                name: name)
        }
        guard let task = idleTask else { return nil }
        return TrackingPillState(mode: .idle, taskID: task.id, name: task.name)
    }

    /// The selected task while it's active, which the pill offers while nothing runs.
    private var idleTask: TaskItem? {
        guard let task = catalog.selectedTask, !task.archived else { return nil }
        return task
    }

    private func finishStop(of name: String) {
        frozen = nil
        if tracking.active == nil, tracking.error == nil {
            announce("Stopped \(name).")
        }
    }
}
