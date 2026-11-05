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
    public var timerText: String
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

    // The text helpers are main-actor isolated because `formatClockDuration` is.
    /// The day's total as the menu shows it, or "Unavailable" while no total is known.
    @MainActor var dayTotalText: String {
        dayTotal.map(formatClockDuration) ?? "Unavailable"
    }

    /// The task's time today as the menu shows it, or "-" while no duration is known.
    @MainActor func durationText(taskID: String) -> String {
        taskDurations[taskID].map(formatClockDuration) ?? "-"
    }
}

/// The stores the menu bar reads.
///
/// Values that combine features, such as whether a task can start, come from `TrackerPresentation` until a
/// later step moves them out of the session.
public struct TrackerMenuSources: Sendable {
    let catalog: TaskCatalogStore
    let connection: ConnectionStore
    let tracking: TrackingStore
    let dailyTotals: DailyTotalsStore
    let menuBar: MenuBarStore
    let presentation: TrackerPresentation

    public init(
        catalog: TaskCatalogStore, connection: ConnectionStore, tracking: TrackingStore,
        dailyTotals: DailyTotalsStore, menuBar: MenuBarStore, presentation: TrackerPresentation
    ) {
        self.catalog = catalog
        self.connection = connection
        self.tracking = tracking
        self.dailyTotals = dailyTotals
        self.menuBar = menuBar
        self.presentation = presentation
    }

    /// Reads every value the menu shows, so an `Observations` closure that calls this tracks all of them.
    func read() -> TrackerMenuInputs {
        TrackerMenuInputs(
            connectionSettings: connection.settings,
            connectionStatusText: connection.statusText,
            isStale: connection.stale,
            tasks: catalog.tasks,
            active: tracking.active,
            timerText: tracking.timerText,
            trackingError: tracking.error,
            runningTaskName: presentation.runningTaskName,
            autoPauseStatusText: presentation.autoPauseStatusText,
            canStopTracking: presentation.canStopTracking,
            startableTaskIDs: presentation.startableTaskIDs,
            dailyTotalsStatus: dailyTotals.status,
            dayTotal: dailyTotals.dayTotal,
            totalsExplanation: dailyTotals.totalsExplanation,
            taskDurations: dailyTotals.taskDurations,
            lastTrackedTaskID: menuBar.lastTrackedTaskID)
    }
}
