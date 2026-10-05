import Foundation

public struct TrackerMenuTask: Identifiable, Equatable {
    public let task: TaskItem
    public let durationText: String
    public let isRunning: Bool
    public let canStart: Bool
    public var id: String { task.id }
}

public struct TrackerMenuContent: Equatable {
    public let connectionStatusText: String
    public let isStale: Bool
    public let runningTaskName: String
    public let autoPauseStatusText: String?
    public let trackingError: String?
    public let activeWorklogID: String?
    public let elapsedText: String?
    public let canStopTracking: Bool
    public let dailyTotalsStatus: DailyTotalsStatus
    public let totalText: String
    public let totalsExplanation: String
    public let todayTasks: [TrackerMenuTask]
    public let otherTasks: [TrackerMenuTask]

    @MainActor
    init(_ session: TrackerSession) {
        connectionStatusText = session.connectionStatusText
        isStale = session.isStale
        runningTaskName = session.runningTaskName
        autoPauseStatusText = session.autoPauseStatusText
        trackingError = session.trackingError
        activeWorklogID = session.active?.id
        elapsedText = session.active == nil ? nil : session.timerDisplayText
        canStopTracking = session.canStopTracking
        dailyTotalsStatus = session.dailyTotalsStatus
        totalText = session.totalDailyDurationText
        totalsExplanation = session.dailyTotalsExplanation
        let today = session.todayTasks
        let todayIDs = Set(today.map(\.id))
        todayTasks = today.map { Self.entry($0, session: session) }
        otherTasks = session.tasks.filter { !$0.archived && !todayIDs.contains($0.id) }
            .sorted {
                if $0.name == $1.name { return $0.id < $1.id }
                return $0.name < $1.name
            }
            .map { Self.entry($0, session: session) }
    }

    @MainActor
    private static func entry(_ task: TaskItem, session: TrackerSession) -> TrackerMenuTask {
        TrackerMenuTask(task: task, durationText: session.dailyDuration(taskID: task.id).map(clockDuration) ?? "-",
                        isRunning: session.active?.taskId == task.id,
                        canStart: session.canStartTracking(taskID: task.id))
    }
}

public struct TrackerMenuLabelContent: Equatable {
    public let symbol: String
    public let status: String
    public let totalText: String?
    public let help: String

    @MainActor
    init(_ session: TrackerSession, showDailyTotal: Bool) {
        if session.isStale {
            symbol = "questionmark.circle"
            status = "Time Tracker: Tracking status unavailable"
        } else if session.active != nil {
            symbol = "play.circle.fill"
            status = "Time Tracker: Tracking \(session.runningTaskName)"
        } else {
            symbol = "clock"
            status = "Time Tracker: \(session.autoPauseStatusText ?? "No timer running")"
        }
        if showDailyTotal {
            let total: String
            if let duration = session.totalDailyDuration {
                let minutes = Int(max(0, duration)) / 60
                let text = String(format: "%02d:%02d", minutes / 60, minutes % 60)
                total = session.dailyTotalsStatus == .cached ? "~\(text)" : text
            } else { total = "-" }
            totalText = total
            help = "\(status)\nTotal today: \(total)\n\(session.dailyTotalsExplanation)"
        } else {
            totalText = nil
            help = status
        }
    }
}
