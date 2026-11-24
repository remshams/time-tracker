import Foundation

/// The values the Quick Track dialog shows, read from the stores in one pass.
///
/// It contains only stored values, so `Observations` sees every change that alters what the dialog shows.
nonisolated struct QuickTrackInputs: Equatable, Sendable {
    var tasks: [TaskItem]
    var active: WorklogItem?
    var elapsedSeconds: Int?
    var isStale: Bool
    var connectionStatusText: String
    var trackingError: String?
    var autoPauseStatusText: String?
    var runningTaskName: String
    var canStopTracking: Bool
    var startableTaskIDs: Set<String>
    var dayTotal: TimeInterval?
    var dailyTotalsStatus: DailyTotalsStatus
    var totalsExplanation: String
    var taskDurations: [String: TimeInterval]

    // The text helpers are main-actor isolated because `DurationText` is.
    /// The running worklog's time, or "Unavailable" before the tracker state is confirmed.
    @MainActor var elapsedText: DurationLineText {
        elapsedSeconds.map { DurationLineText(seconds: $0) } ?? DurationLineText(placeholder: "Unavailable")
    }

    /// The day's total, marked while it's cached, or "Unavailable" while no total is known.
    @MainActor var dayTotalText: DurationLineText {
        dayTotal.map { DurationLineText(seconds: wholeSeconds($0), cached: dailyTotalsStatus == .cached) }
            ?? DurationLineText(placeholder: "Unavailable")
    }

    /// The task's time today, or "-" while no duration is known.
    @MainActor func durationText(taskID: String) -> DurationLineText {
        taskDurations[taskID].map { DurationLineText(seconds: wholeSeconds($0)) } ?? DurationLineText(placeholder: "-")
    }
}

/// The stores the Quick Track dialog reads.
///
/// `TrackingAvailability` decides whether tracking can start or stop. The running task's name is computed here.
struct QuickTrackSources: Sendable {
    let catalog: TaskCatalogStore
    let connection: ConnectionStore
    let tracking: TrackingStore
    let dailyTotals: DailyTotalsStore
    let automation: TrackingAutomationStore
    let availability: TrackingAvailability

    /// Reads every value the dialog shows, so an `Observations` closure that calls this tracks all of them.
    func read() -> QuickTrackInputs {
        QuickTrackInputs(
            tasks: catalog.tasks,
            active: tracking.active,
            elapsedSeconds: tracking.elapsedSeconds,
            isStale: connection.stale,
            connectionStatusText: connection.statusText,
            trackingError: tracking.error,
            autoPauseStatusText: automation.statusText,
            runningTaskName: runningTaskName,
            canStopTracking: availability.canStop,
            startableTaskIDs: availability.startableTaskIDs,
            dayTotal: dailyTotals.dayTotal,
            dailyTotalsStatus: dailyTotals.status,
            totalsExplanation: dailyTotals.totalsExplanation,
            taskDurations: dailyTotals.taskDurations)
    }

    private var runningTaskName: String {
        guard connection.confirmed else { return "Waiting for tracker state" }
        guard let active = tracking.active else { return "No timer running" }
        return catalog.tasks.first { $0.id == active.taskId }?.name ?? "Unknown task"
    }
}
