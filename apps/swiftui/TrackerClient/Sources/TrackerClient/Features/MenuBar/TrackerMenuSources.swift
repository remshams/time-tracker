import Foundation

/// The values the menu bar shows, read from the stores in one pass.
///
/// The menu builds its content, label and live values from this snapshot. It contains only stored values, so
/// `Observations` sees every change that alters what the menu shows.
nonisolated public struct TrackerMenuInputs: Equatable, Sendable {
    public var connectionSettings: ConnectionSettings
    public var connectionStatusText: String
    public var isStale: Bool
    public var tasks: [TaskItem]
    public var active: WorklogItem?
    public var elapsedSeconds: Int?
    public var trackingError: String?
    public var runningTaskName: String
    public var autoPauseStatusText: String?
    public var canStopTracking: Bool
    public var startableTaskIDs: Set<String>
    public var dailyTotalsStatus: DailyTotalsStatus
    public var dayTotal: TimeInterval?
    public var totalsExplanation: String
    public var taskDurations: [String: TimeInterval]
    public var lastTrackedTaskID: String?

    // The text helpers are main-actor isolated because `DurationText` is.
    /// The running worklog's time as the menu shows it, or "Unavailable" before the tracker state is confirmed.
    @MainActor var elapsedText: MenuDurationText {
        elapsedSeconds.map { MenuDurationText(seconds: $0) } ?? MenuDurationText(placeholder: "Unavailable")
    }

    /// The day's total as the menu shows it, marked while it's cached, or "Unavailable" while no total is known.
    @MainActor var dayTotalText: MenuDurationText {
        dayTotal.map { MenuDurationText(seconds: wholeSeconds($0), cached: dailyTotalsStatus == .cached) }
            ?? MenuDurationText(placeholder: "Unavailable")
    }

    /// The task's time today as the menu shows it, or "-" while no duration is known.
    @MainActor func durationText(taskID: String) -> MenuDurationText {
        taskDurations[taskID].map { MenuDurationText(seconds: wholeSeconds($0)) } ?? MenuDurationText(placeholder: "-")
    }
}

/// The stores the menu bar reads.
///
/// `TrackingAvailability` decides whether tracking can start or stop. The running task's name is computed here,
/// because only the menu shows it.
public struct TrackerMenuSources: Sendable {
    let catalog: TaskCatalogStore
    let connection: ConnectionStore
    let tracking: TrackingStore
    let dailyTotals: DailyTotalsStore
    let menuBar: MenuBarStore
    let automation: TrackingAutomationStore
    let availability: TrackingAvailability

    /// Reads every value the menu shows, so an `Observations` closure that calls this tracks all of them.
    func read() -> TrackerMenuInputs {
        TrackerMenuInputs(
            connectionSettings: connection.settings,
            connectionStatusText: connection.statusText,
            isStale: connection.stale,
            tasks: catalog.tasks,
            active: tracking.active,
            elapsedSeconds: tracking.elapsedSeconds,
            trackingError: tracking.error,
            runningTaskName: runningTaskName,
            autoPauseStatusText: automation.statusText,
            canStopTracking: availability.canStop,
            startableTaskIDs: Set(catalog.tasks.map(\.id).filter { availability.canStart(taskID: $0) }),
            dailyTotalsStatus: dailyTotals.status,
            dayTotal: dailyTotals.dayTotal,
            totalsExplanation: dailyTotals.totalsExplanation,
            taskDurations: dailyTotals.taskDurations,
            lastTrackedTaskID: menuBar.lastTrackedTaskID)
    }

    private var runningTaskName: String {
        guard connection.confirmed else { return "Waiting for tracker state" }
        guard let active = tracking.active else { return "No timer running" }
        return catalog.tasks.first { $0.id == active.taskId }?.name ?? "Unknown task"
    }
}
