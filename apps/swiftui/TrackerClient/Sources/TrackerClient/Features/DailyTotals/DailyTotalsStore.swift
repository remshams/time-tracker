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

/// Holds today's task totals for the whole app.
///
/// Views read `status` and each task's `TaskDailyTotal`; the menu bar also reads the day's total, the explanation
/// and each task's duration. `TrackerSync` writes the report and calls `publish` each time it publishes a
/// change, including the one-second display tick and the menu bar's minute tick. `publish` assigns each value
/// only when it changes, so a tick redraws only the running task's label.
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

    init(calendar: Calendar) { self.calendar = calendar }

    @discardableResult
    func updateDay(at date: Date) -> Bool {
        let nextDay = calendar.dateInterval(of: .day, for: date)
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

    func clear(at date: Date) {
        day = nil
        updateDay(at: date)
    }

    func invalidate() {
        if request != nil { loadStatus = .cached }
    }

    func begin(clock: any TrackerClock) -> Request? {
        updateDay(at: clock.now)
        guard let day else { return nil }
        if request == nil { loadStatus = .loading }
        let cutoff = parseTimestamp(formatTimestamp(clock.now)) ?? clock.now
        return Request(day: day, cutoff: cutoff, uptime: clock.uptime, anchorGeneration: anchorGeneration)
    }

    func accept(_ report: TrackerReport, tracking: TrackingObservation, requested: Request, clock: any TrackerClock) {
        updateDay(at: clock.now)
        guard day == requested.day else { return }
        rows = report.rows.reduce(into: [:]) { totals, row in
            totals[row.taskId] = max(0, row.durationMicroseconds)
        }
        let cutoff = report.now.flatMap(parseTimestamp) ?? requested.cutoff
        request = Request(
            day: requested.day, cutoff: cutoff,
            uptime: requested.uptime, anchorGeneration: requested.anchorGeneration)
        reportedActive = tracking.value
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

    func validate(_ report: TrackerReport, tracking: TrackingObservation) throws {
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
        if let active = tracking.value {
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

    func fail(_ failure: Error, clock: any TrackerClock) {
        updateDay(at: clock.now)
        loadStatus = request == nil ? .unavailable : .cached
        error = failure.localizedDescription
    }

    func reanchor(clock: any TrackerClock) {
        anchorGeneration += 1
        guard request != nil else { return }
        anchorDate = clock.now
        anchorUptime = clock.uptime
    }

    func totalDuration(active: WorklogItem?, clock: any TrackerClock) -> TimeInterval? {
        guard let request, calendar.dateInterval(of: .day, for: clock.now) == request.day else { return nil }
        let total = rows.values.reduce(0.0) { $0 + Double($1) } / 1_000_000
        guard let active, let projected = duration(taskID: active.taskId, active: active, clock: clock) else {
            return total
        }
        return total + (projected - baseDuration(taskID: active.taskId))
    }

    func duration(taskID: String, active: WorklogItem?, clock: any TrackerClock) -> TimeInterval? {
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
    func shownStatus(clock: any TrackerClock) -> DailyTotalsStatus {
        guard let day, clock.now >= day.start, clock.now < day.end else { return .unavailable }
        return loadStatus
    }

    func explanation(clock: any TrackerClock) -> String {
        switch shownStatus(clock: clock) {
        case .current: return "Time logged today in your local time zone"
        case .cached: return "Today's total uses cached tracker state. Running time may be unconfirmed."
        case .loading: return "Loading today's totals"
        case .unavailable: return error ?? "Today's total is unavailable"
        }
    }

    /// Shows the current status, the day's total and each task's total. `TrackerSync` calls this each time it
    /// publishes.
    func publish(tasks: [TaskItem], active: WorklogItem?, clock: any TrackerClock) {
        set(\.status, shownStatus(clock: clock))
        set(\.dayTotal, totalDuration(active: active, clock: clock))
        let explanation = explanation(clock: clock)
        set(\.totalsExplanation, explanation)
        let durations: [String: TimeInterval] = tasks.reduce(into: [:]) { totals, task in
            totals[task.id] = duration(taskID: task.id, active: active, clock: clock)
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
}
