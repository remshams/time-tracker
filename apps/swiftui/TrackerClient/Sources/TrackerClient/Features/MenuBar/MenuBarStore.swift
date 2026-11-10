import Foundation
import Observation

public enum MenuPrimaryAction: Equatable, Sendable {
    case stop(worklogID: String)
    case start(taskID: String)
    case openMenu
    case disabled
}

public extension ConnectionSettings {
    var trackingIdentityKey: String {
        mode == .local ? "local" : "server:\(serverURL.trimmingCharacters(in: .whitespacesAndNewlines))"
    }
}

/// Holds the menu bar's own state for the whole app: the last tracked task of the current connection. It also runs
/// the status item's primary action.
///
/// The menu reads `lastTrackedTaskID`; `TrackerSync` calls `observe` after each task list. The task is
/// remembered per connection and saved to the repository, so switching connections back restores it. The menu's
/// copy actions, in `MenuTaskClipboard.swift`, read the task and its time today from the stores passed in here.
@Observable
public final class MenuBarStore: ChangeGuardedStore {
    public private(set) var lastTrackedTaskID: String?
    private let repository: (any LastTrackedTaskRepository)?
    @ObservationIgnored private var remembered: [String: String] = [:]
    let connection: ConnectionStore
    let catalog: TaskCatalogStore
    let tracking: TrackingStore
    let dailyTotals: DailyTotalsStore
    let clock: any TrackerClock
    private let automation: TrackingAutomationStore
    private let availability: TrackingAvailability

    init(
        repository: (any LastTrackedTaskRepository)?, connection: ConnectionStore, catalog: TaskCatalogStore,
        tracking: TrackingStore, dailyTotals: DailyTotalsStore, automation: TrackingAutomationStore,
        availability: TrackingAvailability, clock: any TrackerClock
    ) {
        self.repository = repository
        self.connection = connection
        self.catalog = catalog
        self.tracking = tracking
        self.dailyTotals = dailyTotals
        self.clock = clock
        self.automation = automation
        self.availability = availability
        let settings = connection.settings
        let saved = repository?.load(for: settings)
        lastTrackedTaskID = saved
        remembered[settings.trackingIdentityKey] = saved
    }

    /// What a click on the status item does now: stop the running worklog, start the last tracked task, or open the
    /// menu. Disabled while the screen is locked or no tracking request can be sent.
    var menuPrimaryAction: MenuPrimaryAction {
        guard !automation.locked, availability.canSend else { return .disabled }
        if let active = tracking.active { return .stop(worklogID: active.id) }
        let lastTrackedTask = lastTrackedTaskID.flatMap { id in catalog.tasks.first { $0.id == id } }
        guard let task = lastTrackedTask, !task.archived else { return .openMenu }
        return .start(taskID: task.id)
    }

    /// Runs `menuPrimaryAction` and returns it, so the caller can open the menu for `.openMenu`.
    @discardableResult
    public func performMenuPrimaryAction() -> MenuPrimaryAction {
        let action = menuPrimaryAction
        switch action {
        case .stop(let worklogID): tracking.stopTracking(worklogID: worklogID)
        case .start(let taskID): tracking.startTracking(taskID: taskID)
        case .openMenu, .disabled: break
        }
        return action
    }

    func observe(_ resources: TaskListResources, settings: ConnectionSettings) {
        let key = settings.trackingIdentityKey
        let stored = remembered[key] ?? repository?.load(for: settings)
        let latest = resources.catalog.value.compactMap { task -> (String, Date)? in
            guard let start = parseTimestamp(task.latestStart) else { return nil }
            return (task.id, start)
        }.max(by: lessRecent)
        let idleTaskID: String?
        if let stored {
            if let previous = task(withID: stored, in: resources.catalog.value), let latest,
                isNewer(latest.1, than: previous)
            {
                idleTaskID = latest.0
            } else {
                idleTaskID = stored
            }
        } else {
            idleTaskID = latest?.0
        }
        let taskID = resources.tracking.value?.taskId ?? idleTaskID
        set(\.lastTrackedTaskID, taskID)
        guard let taskID else { return }
        if stored != taskID { repository?.save(taskID: taskID, for: settings) }
        remembered[key] = taskID
    }

    private func lessRecent(_ left: (String, Date), _ right: (String, Date)) -> Bool {
        if sameStart(left.1, right.1) { return largerTaskID(left.0, right.0) }
        return left.1 < right.1
    }

    private func sameStart(_ left: Date, _ right: Date) -> Bool { left == right }

    private func largerTaskID(_ left: String, _ right: String) -> Bool { left > right }

    private func task(withID id: String, in tasks: [TaskItem]) -> TaskItem? {
        for task in tasks {
            if task.id == id { return task }
        }
        return nil
    }

    private func isNewer(_ latest: Date, than previous: TaskItem) -> Bool {
        guard let previousStart = parseTimestamp(previous.latestStart) else { return true }
        return latest > previousStart
    }
}
