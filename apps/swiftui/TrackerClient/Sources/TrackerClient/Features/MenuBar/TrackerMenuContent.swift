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

    init(_ inputs: TrackerMenuInputs) {
        connectionSettings = inputs.connectionSettings
        connectionStatusText = inputs.connectionStatusText
        isStale = inputs.isStale
        runningTaskName = inputs.runningTaskName
        autoPauseStatusText = inputs.autoPauseStatusText
        trackingError = inputs.trackingError
        activeWorklogID = inputs.active?.id
        elapsedText = inputs.active == nil ? nil : inputs.timerText
        canStopTracking = inputs.canStopTracking
        dailyTotalsStatus = inputs.dailyTotalsStatus
        totalText = inputs.dayTotalText
        totalsExplanation = inputs.totalsExplanation
        // Both sections keep the catalog's task order.
        let today = inputs.tasks.filter { (inputs.taskDurations[$0.id] ?? 0) > 0 }
        let todayIDs = Set(today.map(\.id))
        todayTasks = today.map { Self.entry($0, inputs: inputs) }
        otherTasks = inputs.tasks.filter { Self.isOtherTask($0, todayIDs: todayIDs) }
            .map { Self.entry($0, inputs: inputs) }
    }

    private static func isOtherTask(_ task: TaskItem, todayIDs: Set<String>) -> Bool {
        !task.archived && !todayIDs.contains(task.id)
    }

    private static func entry(_ task: TaskItem, inputs: TrackerMenuInputs) -> TrackerMenuTask {
        TrackerMenuTask(
            task: task, durationText: inputs.durationText(taskID: task.id),
            isRunning: inputs.active?.taskId == task.id,
            canStart: inputs.startableTaskIDs.contains(task.id))
    }
}

public struct TrackerMenuLabelContent: Equatable {
    public let indicator: TaskIndicator
    public let status: String
    public let totalText: String?
    public let help: String

    init(_ inputs: TrackerMenuInputs, showDailyTotal: Bool) {
        let lastTrackedTaskID = inputs.lastTrackedTaskID
        let lastTrackedTask = lastTrackedTaskID.flatMap { id in inputs.tasks.first { $0.id == id } }
        indicator = TaskIndicator(
            taskID: inputs.active?.taskId ?? lastTrackedTaskID,
            isRunning: inputs.active != nil, isStale: inputs.isStale)
        if inputs.isStale {
            let name = lastTrackedTask?.name ?? Self.unavailableTaskName(lastTrackedTaskID)
            status =
                name.map { "Tracking status unavailable. Last confirmed task: \($0)" }
                ?? "Tracking status unavailable. No task tracked yet"
        } else if inputs.active != nil {
            status = "Tracking: \(inputs.runningTaskName)"
        } else if let task = lastTrackedTask {
            status = "Stopped. Last tracked: \(task.name)"
        } else if lastTrackedTaskID != nil {
            status = "Stopped. Last tracked task is no longer available"
        } else {
            status = "No task tracked yet"
        }
        if showDailyTotal {
            let total: String
            if let duration = inputs.dayTotal {
                let minutes = Int(max(0, duration)) / 60
                let text = String(format: "%02d:%02d", minutes / 60, minutes % 60)
                total = Self.totalLabel(text, status: inputs.dailyTotalsStatus)
            } else {
                total = "-"
            }
            totalText = total
            help = "\(status)\nTotal today: \(total)\n\(inputs.totalsExplanation)"
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
