import Foundation

public struct TaskDailyTotalPresentation: Equatable, Sendable {
    public let text: String
    public let explanation: String
}

@MainActor
public final class TrackerPresentationObserver {
    public var onContentChange: (() -> Void)?
    public var onActivityChange: (() -> Void)?
    public var onTimerChange: (() -> Void)?
    public var onDailyTotalsChange: (() -> Void)?
    public var onTaskDailyTotalsChange: ((Set<String>) -> Void)?
    private var content: Content
    private var activity: Activity
    private var timerText: String
    private var daily: DailyPresentation
    public var taskDailyTotals: [String: TaskDailyTotalPresentation] { daily.taskTotals }

    public init(session: TrackerSession) {
        content = Content(session)
        activity = Activity(session)
        timerText = session.timerDisplayText
        daily = DailyPresentation(session)
    }

    public func update(from session: TrackerSession) {
        let nextContent = Content(session)
        let nextActivity = Activity(session)
        let nextTimerText = session.timerDisplayText
        let nextDaily = DailyPresentation(session)
        let contentChanged = content != nextContent
        let activityChanged = activity != nextActivity
        let timerChanged = timerText != nextTimerText
        let dailyChanged = daily != nextDaily
        let changedTaskTotals = Set(daily.taskTotals.keys).union(nextDaily.taskTotals.keys)
            .filter { totalChanged(daily.taskTotals[$0], nextDaily.taskTotals[$0]) }
        content = nextContent
        activity = nextActivity
        timerText = nextTimerText
        daily = nextDaily
        if contentChanged { onContentChange?() }
        if activityChanged { onActivityChange?() }
        if timerChanged { onTimerChange?() }
        if dailyChanged { onDailyTotalsChange?() }
        if !changedTaskTotals.isEmpty { onTaskDailyTotalsChange?(changedTaskTotals) }
    }

    private func totalChanged(_ previous: TaskDailyTotalPresentation?, _ current: TaskDailyTotalPresentation?) -> Bool {
        previous != current
    }

    private struct DailyPresentation: Equatable {
        let taskTotals: [String: TaskDailyTotalPresentation]
        let totalText: String
        let status: DailyTotalsStatus
        let error: String?
        let dayStart: Date?

        @MainActor init(_ session: TrackerSession) {
            taskTotals = Dictionary(
                uniqueKeysWithValues: session.tasks.map {
                    (
                        $0.id,
                        TaskDailyTotalPresentation(
                            text: session.dailyDuration(taskID: $0.id).map(clockDuration) ?? "-",
                            explanation: session.dailyTotalsExplanation)
                    )
                })
            totalText = session.totalDailyDurationText
            status = session.dailyTotalsStatus
            error = session.dailyTotalsError
            dayStart = session.dailyTotalsDayStart
        }
    }

    private struct Activity: Equatable {
        let blocksControls: Bool
        let canStart: Bool
        let canStop: Bool

        @MainActor init(_ session: TrackerSession) {
            blocksControls = session.isBlockingControls
            canStart = session.canStartSelectedTask
            canStop = session.canStopTracking
        }
    }

    private struct Content: Equatable {
        let tasks: [TaskItem]
        let active: WorklogItem?
        let worklogs: [WorklogItem]
        let nextCursor: String?
        let error: String?
        let trackingError: String?
        let historyUnavailable: Bool
        let historyLoading: Bool
        let selectedTaskID: String?
        let tab: TaskTab
        let connectionSettings: ConnectionSettings
        let connectionStatus: String
        let connectionMessage: String?
        let changingConnection: Bool
        let stale: Bool
        let pauseOnScreenLock: Bool
        let autoPauseStatus: String?
        let todayTaskIDs: [String]
        let dailyStatus: DailyTotalsStatus
        let dailyError: String?
        let dayStart: Date?

        @MainActor init(_ session: TrackerSession) {
            tasks = session.tasks
            active = session.active
            worklogs = session.worklogs
            nextCursor = session.nextCursor
            error = session.error
            trackingError = session.trackingError
            historyUnavailable = session.historyUnavailable
            historyLoading = session.isHistoryLoading
            selectedTaskID = session.selectedTaskID
            tab = session.tab
            connectionSettings = session.connectionSettings
            connectionStatus = session.connectionStatusText
            connectionMessage = session.connectionMessage
            changingConnection = session.isChangingConnection
            stale = session.isStale
            pauseOnScreenLock = session.pauseOnScreenLock
            autoPauseStatus = session.autoPauseStatusText
            todayTaskIDs = session.todayTasks.map(\.id)
            dailyStatus = session.dailyTotalsStatus
            dailyError = session.dailyTotalsError
            dayStart = session.dailyTotalsDayStart
        }
    }
}
