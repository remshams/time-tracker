import Foundation
import Observation

public struct TaskDailyTotalPresentation: Equatable, Sendable {
    public let text: String
    public let explanation: String
}

/// Holds one task's daily total so a ticking timer redraws only that task's label.
@Observable
public final class TaskDailyTotal {
    public fileprivate(set) var content: TaskDailyTotalPresentation?

    fileprivate init(content: TaskDailyTotalPresentation?) { self.content = content }
}

/// Loads today's task totals and holds them for the whole app.
///
/// Views read `status` and each task's `TaskDailyTotal`; the menu bar also reads the day's total, the explanation
/// and each task's duration. The store requests the report, checks it and records a failure, and starts the next
/// day at midnight. `TrackerSync` decides when to load and calls `publish` each time it publishes a change,
/// including the one-second display tick and the menu bar's minute tick. `publish` assigns each value only when it
/// changes, so a tick redraws only the running task's label.
@Observable
public final class DailyTotalsStore: ChangeGuardedStore {
    struct Request {
        let day: DateInterval
        let cutoff: Date
        let uptime: TimeInterval
        let anchorGeneration: Int
        var start: String { formatTimestamp(day.start) }
        var end: String { formatTimestamp(day.end) }
        var now: String { formatTimestamp(cutoff) }
    }

    /// The status views show; `.unavailable` once the loaded day has ended.
    public private(set) var status: DailyTotalsStatus = .unavailable
    /// Today's total including the running worklog, or `nil` while no report covers today.
    public private(set) var dayTotal: TimeInterval?
    /// Explains where today's totals come from, or why they are unavailable.
    public private(set) var totalsExplanation = "Today's total is unavailable"
    /// Today's duration for each catalog task that a report covers.
    public private(set) var taskDurations: [String: TimeInterval] = [:]
    /// Takes in the task list that came with a report. `TrackerFeatures` sets it to `TrackerSync`'s task list
    /// handling, which publishes.
    @ObservationIgnored var acceptReportedTaskList: @MainActor (TaskListResources) -> Void = { _ in }
    /// Receives a failed report's error, so that `TrackerSync` can recover the connection. `TrackerFeatures` sets it.
    @ObservationIgnored var recordReadFailure: @MainActor (Error) -> Void = { _ in }
    /// Reloads the task list when a new day starts. `TrackerFeatures` sets it to `TrackerSync`'s refresh.
    @ObservationIgnored var refreshTaskList: @MainActor () -> Void = {}
    /// Shows the totals and the other derived values. `TrackerFeatures` sets it to `TrackerSync`'s publish.
    @ObservationIgnored var publishChanges: @MainActor () -> Void = {}
    private let reports: (any ReportClient)?
    private let connection: ConnectionStore
    private let clock: any TrackerClock
    private let scheduler: any TrackerScheduler
    private let calendar: Calendar
    // Views observe only the public values and each task's model. Ignoring the rest keeps a tick from redrawing
    // views that read the store, such as the sidebar that looks up task models with `taskTotal(taskID:)`.
    @ObservationIgnored private var request: Request?
    @ObservationIgnored private var rows: [String: Int64] = [:]
    @ObservationIgnored private var reportedActive: WorklogItem?
    @ObservationIgnored private var anchorDate: Date?
    @ObservationIgnored private var anchorUptime: TimeInterval = 0
    @ObservationIgnored private var anchorGeneration = 0
    @ObservationIgnored private(set) var day: DateInterval?
    @ObservationIgnored private(set) var loadStatus: DailyTotalsStatus = .unavailable
    @ObservationIgnored private(set) var error: String?
    @ObservationIgnored private var taskTotals: [String: TaskDailyTotalPresentation] = [:]
    @ObservationIgnored private var taskTotalModels: [String: TaskDailyTotal] = [:]
    // The rollover timer runs between `start` or `wake` and `sleep`. The scheduler runs no callback after cancel().
    @ObservationIgnored private var rolloverTimer: (any TrackerCancellation)?
    @ObservationIgnored private var running = false

    init(
        reports: (any ReportClient)?, connection: ConnectionStore, clock: any TrackerClock,
        scheduler: any TrackerScheduler, calendar: Calendar
    ) {
        self.reports = reports
        self.connection = connection
        self.clock = clock
        self.scheduler = scheduler
        self.calendar = calendar
        updateDay()
    }

    isolated deinit {
        rolloverTimer?.cancel()
    }

    /// Whether loading the report also loads the task list, which needs a report client.
    var loadsTaskList: Bool { reports != nil }

    /// Starts the day's rollover timer. `TrackerSync` calls this when it starts.
    func start() {
        running = true
        updateDay()
        scheduleRollover()
    }

    /// Stops the rollover timer while the computer sleeps.
    func sleep() {
        running = false
        cancelRollover()
    }

    /// Moves to the current day, measures the running worklog from the current time and restarts the rollover timer.
    func wake() {
        running = true
        updateDay()
        reanchor()
        scheduleRollover()
    }

    /// Takes in a loaded task list. When a report follows, the loaded totals become cached until it arrives.
    func acceptTaskList(reportRequested: Bool) {
        updateDay()
        // Only an accepted report sets the request, so totals become cached only with a report client.
        if reportRequested && request != nil { loadStatus = .cached }
        scheduleRollover()
    }

    /// Starts the next day when the current one has ended, which the display tick checks each second.
    func rollOverIfDayEnded() {
        guard reports != nil, updateDay() else { return }
        rollOver()
    }

    /// Requests today's report, which also returns the task list. Does nothing without a report client. Otherwise
    /// publishes on every path: before a first request, through the accepted task list, and after a failure.
    func load() async {
        guard let reports, let requested = begin() else { return }
        // A first request shows the loading status, which the menu reads from the store.
        publishChanges()
        do {
            let refreshed = try await reports.refreshTaskListWithTotals(
                settings: connection.settings, start: requested.start,
                end: requested.end, now: requested.now)
            try validate(refreshed)
            accept(refreshed, requested: requested)
            acceptReportedTaskList(refreshed.taskList)
        } catch {
            fail(error)
            recordReadFailure(error)
            publishChanges()
        }
    }

    @discardableResult
    private func updateDay() -> Bool {
        let nextDay = calendar.dateInterval(of: .day, for: clock.now)
        guard nextDay != day else { return false }
        day = nextDay
        rows = [:]
        request = nil
        reportedActive = nil
        anchorDate = nil
        loadStatus = .unavailable
        error = nil
        return true
    }

    /// Drops the totals of the previous connection.
    func acceptConnection() {
        day = nil
        updateDay()
    }

    func begin() -> Request? {
        updateDay()
        guard let day else { return nil }
        if request == nil { loadStatus = .loading }
        let cutoff = parseTimestamp(formatTimestamp(clock.now)) ?? clock.now
        return Request(day: day, cutoff: cutoff, uptime: clock.uptime, anchorGeneration: anchorGeneration)
    }

    func accept(_ refresh: TaskListTotalsRefresh, requested: Request) {
        updateDay()
        guard day == requested.day else { return }
        let report = refresh.report
        rows = report.rows.reduce(into: [:]) { totals, row in
            totals[row.taskId] = max(0, row.durationMicroseconds)
        }
        let cutoff = report.now.flatMap(parseTimestamp) ?? requested.cutoff
        request = Request(
            day: requested.day, cutoff: cutoff,
            uptime: requested.uptime, anchorGeneration: requested.anchorGeneration)
        reportedActive = refresh.taskList.tracking.value
        if requested.anchorGeneration == anchorGeneration {
            anchorDate = cutoff
            anchorUptime = requested.uptime
        } else {
            anchorDate = clock.now
            anchorUptime = clock.uptime
        }
        loadStatus = .current
        error = nil
    }

    func validate(_ refresh: TaskListTotalsRefresh) throws {
        let report = refresh.report
        if let now = report.now, parseTimestamp(now) == nil {
            throw BridgeFailure(
                message: "The report contains an invalid cutoff.", kind: "protocol", requiresRefresh: true)
        }
        var rowIDs = Set<String>()
        for row in report.rows {
            guard validTotal(row), rowIDs.insert(row.taskId).inserted
            else {
                throw BridgeFailure(
                    message: "The report contains invalid task totals.", kind: "protocol", requiresRefresh: true)
            }
        }
        if let active = refresh.taskList.tracking.value {
            guard validRunningWorklog(active) else {
                throw BridgeFailure(
                    message: "The report contains invalid running worklog data.", kind: "protocol",
                    requiresRefresh: true)
            }
        }
    }

    private func validTotal(_ row: TaskReportTotal) -> Bool {
        !row.taskId.isEmpty && row.durationMicroseconds >= 0
    }

    private func validRunningWorklog(_ worklog: WorklogItem) -> Bool {
        !worklog.id.isEmpty && !worklog.taskId.isEmpty && worklog.end == nil && parseTimestamp(worklog.start) != nil
    }

    func fail(_ failure: Error) {
        updateDay()
        loadStatus = request == nil ? .unavailable : .cached
        error = failure.localizedDescription
    }

    func reanchor() {
        anchorGeneration += 1
        guard request != nil else { return }
        anchorDate = clock.now
        anchorUptime = clock.uptime
    }

    func totalDuration(active: WorklogItem?) -> TimeInterval? {
        guard let request, calendar.dateInterval(of: .day, for: clock.now) == request.day else { return nil }
        let total = rows.values.reduce(0.0) { $0 + Double($1) } / 1_000_000
        guard let active, let projected = duration(taskID: active.taskId, active: active) else {
            return total
        }
        return total + (projected - baseDuration(taskID: active.taskId))
    }

    func duration(taskID: String, active: WorklogItem?) -> TimeInterval? {
        guard let request, calendar.dateInterval(of: .day, for: clock.now) == request.day else { return nil }
        let base = baseDuration(taskID: taskID)
        guard let active, let reportedActive, active.id == reportedActive.id,
            active.taskId == reportedActive.taskId, active.start == reportedActive.start,
            active.taskId == taskID, active.end == nil,
            let start = parseTimestamp(active.start), let anchorDate
        else { return base }
        let projectedNow = anchorDate.addingTimeInterval(max(0, clock.uptime - anchorUptime))
        let lower = max(request.cutoff, request.day.start, start)
        let upper = min(projectedNow, request.day.end)
        return base + max(0, upper.timeIntervalSince(lower))
    }

    /// The loaded status while its day lasts; `.unavailable` once the day has ended.
    func shownStatus() -> DailyTotalsStatus {
        isToday ? loadStatus : .unavailable
    }

    /// Whether the loaded day is still today.
    private var isToday: Bool {
        guard let day else { return false }
        return clock.now >= day.start && clock.now < day.end
    }

    func explanation() -> String {
        switch shownStatus() {
        case .current: return "Time logged today in your local time zone"
        case .cached: return "Today's total uses cached tracker state. Running time may be unconfirmed."
        case .loading: return "Loading today's totals"
        // An error belongs to its day, so the previous day's error disappears at midnight.
        case .unavailable: return (isToday ? error : nil) ?? "Today's total is unavailable"
        }
    }

    /// Shows the current status, the day's total and each task's total. `TrackerSync` calls this each time it
    /// publishes.
    func publish(tasks: [TaskItem], active: WorklogItem?) {
        set(\.status, shownStatus())
        set(\.dayTotal, totalDuration(active: active))
        let explanation = explanation()
        set(\.totalsExplanation, explanation)
        let durations: [String: TimeInterval] = tasks.reduce(into: [:]) { totals, task in
            totals[task.id] = duration(taskID: task.id, active: active)
        }
        set(\.taskDurations, durations)
        taskTotals = Dictionary(
            uniqueKeysWithValues: tasks.map {
                (
                    $0.id,
                    TaskDailyTotalPresentation(
                        text: durations[$0.id].map(formatClockDuration) ?? "-", explanation: explanation)
                )
            })
        for (taskID, model) in taskTotalModels {
            if let content = taskTotals[taskID] {
                if model.content != content { model.content = content }
            } else {
                taskTotalModels.removeValue(forKey: taskID)
            }
        }
    }

    /// Returns the task's observable total. Totals for tasks that leave the catalog stop updating.
    public func taskTotal(taskID: String) -> TaskDailyTotal {
        if let existing = taskTotalModels[taskID] { return existing }
        let model = TaskDailyTotal(content: taskTotals[taskID])
        taskTotalModels[taskID] = model
        return model
    }

    private func baseDuration(taskID: String) -> TimeInterval {
        Double(rows[taskID] ?? 0) / 1_000_000
    }

    /// Restarts the rollover timer and reloads the task list and today's report, unless the server is incompatible.
    private func rollOver() {
        scheduleRollover()
        if !connection.protocolBlocked { refreshTaskList() }
    }

    private func scheduleRollover() {
        cancelRollover()
        guard reports != nil, running, let day else { return }
        rolloverTimer = scheduler.schedule(
            after: max(0.001, day.end.timeIntervalSince(clock.now)),
            repeating: false, tolerance: 0
        ) { [weak self] in
            guard let self else { return }
            updateDay()
            rollOver()
            publishChanges()
        }
    }

    private func cancelRollover() {
        rolloverTimer?.cancel()
        rolloverTimer = nil
    }
}
