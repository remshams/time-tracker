import Foundation

@MainActor
public final class TrackerPresentationObserver {
    public var onContentChange: (() -> Void)?
    public var onActivityChange: (() -> Void)?
    public var onTimerChange: (() -> Void)?
    public var onDailyTotalsChange: (() -> Void)?
    public var onTaskCreationChange: (() -> Void)?

    private var content: Content
    private var activity: Activity
    private var timerText: String
    private var daily: DailyPresentation
    private var creation: TaskCreationPresentation
    private var canOpenCreation: Bool

    public init(session: TrackerSession) {
        content = Content(session)
        activity = Activity(session)
        timerText = session.timerDisplayText
        daily = DailyPresentation(session)
        creation = session.taskCreation
        canOpenCreation = session.canOpenTaskCreation
    }

    public func update(from session: TrackerSession) {
        let nextContent = Content(session)
        let nextActivity = Activity(session)
        let nextTimerText = session.timerDisplayText
        let nextDaily = DailyPresentation(session)
        let nextCreation = session.taskCreation
        let nextCanOpenCreation = session.canOpenTaskCreation
        let contentChanged = content != nextContent
        let activityChanged = activity != nextActivity
        let timerChanged = timerText != nextTimerText
        let dailyChanged = daily != nextDaily
        let creationChanged = creation != nextCreation || canOpenCreation != nextCanOpenCreation
        content = nextContent
        activity = nextActivity
        timerText = nextTimerText
        daily = nextDaily
        creation = nextCreation
        canOpenCreation = nextCanOpenCreation
        if contentChanged { onContentChange?() }
        if activityChanged { onActivityChange?() }
        if timerChanged { onTimerChange?() }
        if dailyChanged { onDailyTotalsChange?() }
        if creationChanged { onTaskCreationChange?() }
    }

    private struct DailyPresentation: Equatable {
        let texts: [String: String]
        let totalText: String
        let status: DailyTotalsStatus
        let error: String?
        let dayStart: Date?

        @MainActor init(_ session: TrackerSession) {
            texts = Dictionary(uniqueKeysWithValues: session.tasks.map { ($0.id, session.dailyDurationText(taskID: $0.id)) })
            totalText = session.totalDailyDurationText
            status = session.dailyTotalsStatus
            error = session.dailyTotalsError
            dayStart = session.dailyTotalsDayStart
        }
    }

    private struct Activity: Equatable {
        let busy: Bool
        let canStart: Bool
        let canStop: Bool

        @MainActor init(_ session: TrackerSession) {
            busy = session.isBusy
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
