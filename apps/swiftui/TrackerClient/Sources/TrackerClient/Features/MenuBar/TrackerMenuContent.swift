import Foundation

public struct TrackerMenuTask: Identifiable, Equatable {
    public let task: TaskItem
    public let durationText: String
    public let isRunning: Bool
    public let canStart: Bool
    public var id: String { task.id }
}

public struct TrackerMenuContent: Equatable {
    public let connectionSettings: ConnectionSettings
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
        connectionSettings = session.connectionSettings
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
        otherTasks = session.tasks.filter { Self.isOtherTask($0, todayIDs: todayIDs) }
            .sorted(by: Self.isOrderedBefore)
            .map { Self.entry($0, session: session) }
    }

    private static func isOtherTask(_ task: TaskItem, todayIDs: Set<String>) -> Bool {
        !task.archived && !todayIDs.contains(task.id)
    }

    private static func isOrderedBefore(_ left: TaskItem, _ right: TaskItem) -> Bool {
        if left.name == right.name { return isEarlierID(left, right) }
        return left.name < right.name
    }

    private static func isEarlierID(_ left: TaskItem, _ right: TaskItem) -> Bool {
        left.id < right.id
    }

    @MainActor
    private static func entry(_ task: TaskItem, session: TrackerSession) -> TrackerMenuTask {
        TrackerMenuTask(
            task: task, durationText: session.dailyDuration(taskID: task.id).map(clockDuration) ?? "-",
            isRunning: session.active?.taskId == task.id,
            canStart: session.canStartTracking(taskID: task.id))
    }
}

public struct TrackerMenuLabelContent: Equatable {
    public let indicator: TaskIndicator
    public var taskColor: TaskColor? { indicator.color }
    public var symbol: String { indicator.symbol }
    public let status: String
    public let totalText: String?
    public let help: String

    @MainActor
    init(_ session: TrackerSession, showDailyTotal: Bool) {
        indicator = TaskIndicator(
            taskID: session.active?.taskId ?? session.lastTrackedTaskID,
            isRunning: session.active != nil, isStale: session.isStale)
        if session.isStale {
            let name = session.lastTrackedTask?.name ?? Self.unavailableTaskName(session.lastTrackedTaskID)
            status =
                name.map { "Tracking status unavailable. Last confirmed task: \($0)" }
                ?? "Tracking status unavailable. No task tracked yet"
        } else if session.active != nil {
            status = "Tracking: \(session.runningTaskName)"
        } else if let task = session.lastTrackedTask {
            status = "Stopped. Last tracked: \(task.name)"
        } else if session.lastTrackedTaskID != nil {
            status = "Stopped. Last tracked task is no longer available"
        } else {
            status = "No task tracked yet"
        }
        if showDailyTotal {
            let total: String
            if let duration = session.totalDailyDuration {
                let minutes = Int(max(0, duration)) / 60
                let text = String(format: "%02d:%02d", minutes / 60, minutes % 60)
                total = Self.totalLabel(text, status: session.dailyTotalsStatus)
            } else {
                total = "-"
            }
            totalText = total
            help = "\(status)\nTotal today: \(total)\n\(session.dailyTotalsExplanation)"
        } else {
            totalText = nil
            help = status
        }
    }

    private static func unavailableTaskName(_ taskID: String?) -> String? {
        taskID == nil ? nil : "Unavailable task"
    }

    private static func totalLabel(_ text: String, status: DailyTotalsStatus) -> String {
        status == .cached ? "~\(text)" : text
    }
}
