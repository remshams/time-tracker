import Foundation

@MainActor
public final class TrackerPresentationObserver {
    public var onContentChange: (() -> Void)?
    public var onActivityChange: (() -> Void)?
    public var onTimerChange: (() -> Void)?

    private var content: Content
    private var activity: Activity
    private var timerText: String

    public init(session: TrackerSession) {
        content = Content(session)
        activity = Activity(session)
        timerText = session.timerDisplayText
    }

    public func update(from session: TrackerSession) {
        let nextContent = Content(session)
        let nextActivity = Activity(session)
        let nextTimerText = session.timerDisplayText
        let contentChanged = content != nextContent
        let activityChanged = activity != nextActivity
        let timerChanged = timerText != nextTimerText
        content = nextContent
        activity = nextActivity
        timerText = nextTimerText
        if contentChanged { onContentChange?() }
        if activityChanged { onActivityChange?() }
        if timerChanged { onTimerChange?() }
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
        }
    }
}
