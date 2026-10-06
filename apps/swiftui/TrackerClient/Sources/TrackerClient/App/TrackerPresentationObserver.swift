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
    public var onTaskCreationChange: (() -> Void)?
    public var onTaskArchivingChange: (() -> Void)?
    public private(set) var taskArchivingSheetContent: TaskArchivingPresentation
    private var archiving: TaskArchivingPresentation
    private var archiveAvailability: [String: Bool]
    public var onTaskRenameChange: (() -> Void)?
    public var onWorklogMoveChange: (() -> Void)?
    public private(set) var worklogMoveSheetContent: WorklogMovePresentation
    private var move: WorklogMovePresentation
    private var canOpenMove: Bool
    public var onWorklogCorrectionChange: (() -> Void)?
    public private(set) var worklogCorrectionSheetContent: WorklogCorrectionPresentation

    private var content: Content
    private var activity: Activity
    private var timerText: String
    private var daily: DailyPresentation
    private var creation: TaskCreationPresentation
    private var canOpenCreation: Bool
    private var rename: TaskRenamePresentation
    private var canOpenRename: Bool
    private var correction: WorklogCorrectionPresentation
    private var canOpenCorrection: Bool

    public var taskDailyTotals: [String: TaskDailyTotalPresentation] { daily.taskTotals }

    public init(session: TrackerSession) {
        archiving = session.taskArchiving
        taskArchivingSheetContent = archiving
        archiveAvailability = session.taskArchivingAvailability
        move = session.worklogMove
        canOpenMove = session.canOpenWorklogMove
        worklogMoveSheetContent = move
        content = Content(session)
        activity = Activity(session)
        timerText = session.timerDisplayText
        daily = DailyPresentation(session)
        creation = session.taskCreation
        canOpenCreation = session.canOpenTaskCreation
        rename = session.taskRename
        canOpenRename = session.canOpenTaskRename
        correction = session.worklogCorrection
        worklogCorrectionSheetContent = correction
        canOpenCorrection = session.canOpenWorklogCorrection
    }

    public func update(from session: TrackerSession) {
        let nextArchiving = session.taskArchiving
        let nextArchiveAvailability = session.taskArchivingAvailability
        let archivingChanged = archiving != nextArchiving || archiveAvailability != nextArchiveAvailability
        archiving = nextArchiving
        archiveAvailability = nextArchiveAvailability
        if nextArchiving.isPresented { taskArchivingSheetContent = nextArchiving }
        let nextMove = session.worklogMove
        let nextCanOpenMove = session.canOpenWorklogMove
        let moveChanged = move != nextMove || canOpenMove != nextCanOpenMove
        move = nextMove
        canOpenMove = nextCanOpenMove
        if nextMove.isPresented { worklogMoveSheetContent = nextMove }
        let nextContent = Content(session)
        let nextActivity = Activity(session)
        let nextTimerText = session.timerDisplayText
        let nextDaily = DailyPresentation(session)
        let nextCreation = session.taskCreation
        let nextCanOpenCreation = session.canOpenTaskCreation
        let nextRename = session.taskRename
        let nextCanOpenRename = session.canOpenTaskRename
        let nextCorrection = session.worklogCorrection
        let nextCanOpenCorrection = session.canOpenWorklogCorrection
        let contentChanged = content != nextContent
        let activityChanged = activity != nextActivity
        let timerChanged = timerText != nextTimerText
        let dailyChanged = daily != nextDaily
        let changedTaskTotals = Set(daily.taskTotals.keys).union(nextDaily.taskTotals.keys)
            .filter { daily.taskTotals[$0] != nextDaily.taskTotals[$0] }
        let creationChanged = creation != nextCreation || canOpenCreation != nextCanOpenCreation
        let renameChanged = rename != nextRename || canOpenRename != nextCanOpenRename
        let correctionChanged = correction != nextCorrection || canOpenCorrection != nextCanOpenCorrection
        content = nextContent
        activity = nextActivity
        timerText = nextTimerText
        daily = nextDaily
        creation = nextCreation
        canOpenCreation = nextCanOpenCreation
        rename = nextRename
        canOpenRename = nextCanOpenRename
        correction = nextCorrection
        // SwiftUI continues rendering the sheet while its dismissal animates.
        if nextCorrection.isPresented { worklogCorrectionSheetContent = nextCorrection }
        canOpenCorrection = nextCanOpenCorrection
        if archivingChanged { onTaskArchivingChange?() }
        if moveChanged { onWorklogMoveChange?() }
        if contentChanged { onContentChange?() }
        if activityChanged { onActivityChange?() }
        if timerChanged { onTimerChange?() }
        if dailyChanged { onDailyTotalsChange?() }
        if !changedTaskTotals.isEmpty { onTaskDailyTotalsChange?(changedTaskTotals) }
        if creationChanged { onTaskCreationChange?() }
        if renameChanged { onTaskRenameChange?() }
        if correctionChanged { onWorklogCorrectionChange?() }
    }

    private struct DailyPresentation: Equatable {
        let taskTotals: [String: TaskDailyTotalPresentation]
        let totalText: String
        let status: DailyTotalsStatus
        let error: String?
        let dayStart: Date?

        @MainActor init(_ session: TrackerSession) {
            taskTotals = Dictionary(uniqueKeysWithValues: session.tasks.map {
                ($0.id, TaskDailyTotalPresentation(text: session.dailyDuration(taskID: $0.id).map(clockDuration) ?? "-",
                                                   explanation: session.dailyTotalsExplanation))
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
