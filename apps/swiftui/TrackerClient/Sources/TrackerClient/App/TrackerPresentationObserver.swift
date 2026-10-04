import Foundation

@MainActor
public final class TrackerPresentationObserver {
    public var onContentChange: (() -> Void)?
    public var onActivityChange: (() -> Void)?
    public var onTimerChange: (() -> Void)?
    public var onDailyTotalsChange: (() -> Void)?

    private var content: Content
    private var activity: Activity
    private var timerText: String
    private var dailyTexts: [String: String]

    public init(session: TrackerSession) {
        content = Content(session)
        activity = Activity(session)
        timerText = session.timerDisplayText
        dailyTexts = Self.dailyTexts(session)
    }

    public func update(from session: TrackerSession) {
        let nextContent = Content(session)
        let nextActivity = Activity(session)
        let nextTimerText = session.timerDisplayText
        let nextDailyTexts = Self.dailyTexts(session)
        let contentChanged = content != nextContent
        let activityChanged = activity != nextActivity
        let timerChanged = timerText != nextTimerText
        let dailyChanged = dailyTexts != nextDailyTexts || content.dailyStatus != nextContent.dailyStatus ||
            content.dailyError != nextContent.dailyError || content.dayStart != nextContent.dayStart
        content = nextContent
        activity = nextActivity
        timerText = nextTimerText
        dailyTexts = nextDailyTexts
        if contentChanged { onContentChange?() }
        if activityChanged { onActivityChange?() }
        if timerChanged { onTimerChange?() }
        if dailyChanged { onDailyTotalsChange?() }
    }

    private static func dailyTexts(_ session: TrackerSession) -> [String: String] {
        Dictionary(uniqueKeysWithValues: session.tasks.map { ($0.id, session.dailyDurationText(taskID: $0.id)) })
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
