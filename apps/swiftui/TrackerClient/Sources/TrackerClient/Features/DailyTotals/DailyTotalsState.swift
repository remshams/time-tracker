import Foundation

@MainActor
final class DailyTotalsState {
    struct Request {
        let day: DateInterval
        let cutoff: Date
        let uptime: TimeInterval
        let anchorGeneration: Int
        var start: String { commandTimestamp(day.start) }
        var end: String { commandTimestamp(day.end) }
        var now: String { commandTimestamp(cutoff) }
    }

    private let calendar: Calendar
    private var request: Request?
    private var rows: [String: Int64] = [:]
    private var reportedActive: WorklogItem?
    private var anchorDate: Date?
    private var anchorUptime: TimeInterval = 0
    private var anchorGeneration = 0
    private(set) var day: DateInterval?
    private(set) var status: DailyTotalsStatus = .unavailable
    private(set) var error: String?
    var pending = false

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
        status = .unavailable
        error = nil
        pending = true
        return true
    }

    func clear(at date: Date) {
        day = nil
        updateDay(at: date)
    }

    func invalidate() {
        if request != nil { status = .cached }
        pending = true
    }

    func begin(clock: any TrackerClock) -> Request? {
        updateDay(at: clock.now)
        pending = false
        guard let day else { return nil }
        if request == nil { status = .loading }
        let cutoff = timestamp(commandTimestamp(clock.now)) ?? clock.now
        return Request(day: day, cutoff: cutoff, uptime: clock.uptime, anchorGeneration: anchorGeneration)
    }

    func accept(_ report: TrackerReport, tracking: TrackingObservation?, requested: Request, clock: any TrackerClock) {
        updateDay(at: clock.now)
        guard day == requested.day else { pending = true; return }
        rows = report.rows.reduce(into: [:]) { totals, row in
            totals[row.taskId] = max(0, row.durationMicroseconds)
        }
        request = Request(
            day: requested.day, cutoff: report.now.flatMap(timestamp) ?? requested.cutoff,
            uptime: requested.uptime, anchorGeneration: requested.anchorGeneration)
        reportedActive = report.revision == tracking?.revision ? tracking?.value : nil
        let cutoff = report.now.flatMap(timestamp) ?? requested.cutoff
        if requested.anchorGeneration == anchorGeneration {
            anchorDate = cutoff
            anchorUptime = requested.uptime
        } else {
            anchorDate = clock.now
            anchorUptime = clock.uptime
        }
        status = tracking != nil && report.revision == tracking?.revision ? .current : .cached
        error = nil
    }

    func validate(_ report: TrackerReport, tracking: TrackingObservation? = nil) throws {
        if let now = report.now, timestamp(now) == nil {
            throw BridgeFailure(
                message: "The report contains an invalid cutoff.", kind: "protocol", requiresRefresh: true)
        }
        var rowIDs = Set<String>()
        for row in report.rows {
            guard !row.taskId.isEmpty, row.durationMicroseconds >= 0,
                rowIDs.insert(row.taskId).inserted
            else {
                throw BridgeFailure(
                    message: "The report contains invalid task totals.", kind: "protocol", requiresRefresh: true)
            }
        }
        if let active = tracking?.value {
            guard !active.id.isEmpty, !active.taskId.isEmpty, active.end == nil, timestamp(active.start) != nil
            else {
                throw BridgeFailure(
                    message: "The report contains invalid running worklog data.", kind: "protocol",
                    requiresRefresh: true)
            }
        }
    }

    func fail(_ failure: Error, clock: any TrackerClock) {
        updateDay(at: clock.now)
        status = request == nil ? .unavailable : .cached
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
            let start = timestamp(active.start), let anchorDate
        else { return base }
        let projectedNow = anchorDate.addingTimeInterval(max(0, clock.uptime - anchorUptime))
        let lower = max(request.cutoff, request.day.start, start)
        let upper = min(projectedNow, request.day.end)
        return base + max(0, upper.timeIntervalSince(lower))
    }

    private func baseDuration(taskID: String) -> TimeInterval {
        Double(rows[taskID] ?? 0) / 1_000_000
    }
}
