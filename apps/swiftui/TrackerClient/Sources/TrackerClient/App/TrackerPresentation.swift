import Foundation
import Observation

public struct TaskDailyTotalPresentation: Equatable, Sendable {
    public let text: String
    public let explanation: String
}

/// Holds one task's daily total so a ticking timer redraws only that task's label.
@MainActor
@Observable
public final class TaskDailyTotal {
    public fileprivate(set) var content: TaskDailyTotalPresentation?

    fileprivate init(content: TaskDailyTotalPresentation?) { self.content = content }
}

/// Copies the session values that views read into observable properties.
///
/// Observation notifies on every assignment, including an equal value. `update(from:)` assigns a
/// property only when its value changes, so draft edits, clock ticks, and unchanged polls notify
/// only the views that read the changed values.
@MainActor
@Observable
public final class TrackerPresentation {
    public private(set) var active: WorklogItem?
    public private(set) var worklogs: [WorklogItem]
    public private(set) var hasMoreHistory: Bool
    public private(set) var error: String?
    public private(set) var trackingError: String?
    public private(set) var historyUnavailable: Bool
    public private(set) var isHistoryLoading: Bool
    public private(set) var visibleTasks: [TaskItem]
    public private(set) var selectedTaskID: String?
    public private(set) var selectedTask: TaskItem?
    public private(set) var tab: TaskTab
    public private(set) var connectionSettings: ConnectionSettings
    public private(set) var connectionStatusText: String
    public private(set) var connectionMessage: String?
    public private(set) var isChangingConnection: Bool
    public private(set) var isStale: Bool
    public private(set) var pauseOnScreenLock: Bool
    public private(set) var autoPauseStatusText: String?
    public private(set) var dailyTotalsStatus: DailyTotalsStatus

    public private(set) var isBlockingControls: Bool
    public private(set) var canStopTracking: Bool
    public private(set) var startableTaskIDs: Set<String>
    public private(set) var timerDisplayText: String

    public private(set) var taskCreation: TaskCreationPresentation
    public private(set) var canOpenTaskCreation: Bool
    public private(set) var taskRename: TaskRenamePresentation
    public private(set) var canOpenTaskRename: Bool
    public private(set) var taskArchiving: TaskArchivingPresentation
    public private(set) var taskArchivingSheetContent: TaskArchivingPresentation
    public private(set) var archivableTaskIDs: Set<String>
    public private(set) var unarchivableTaskIDs: Set<String>
    public private(set) var bulkTaskArchiving: BulkTaskArchivingPresentation
    public private(set) var bulkTaskArchivingSheetContent: BulkTaskArchivingPresentation
    public private(set) var canOpenBulkTaskArchiving: Bool
    public private(set) var worklogCorrection: WorklogCorrectionPresentation
    public private(set) var worklogCorrectionSheetContent: WorklogCorrectionPresentation
    public private(set) var canOpenWorklogCorrection: Bool
    public private(set) var worklogMove: WorklogMovePresentation
    public private(set) var worklogMoveSheetContent: WorklogMovePresentation
    public private(set) var canOpenWorklogMove: Bool
    public private(set) var worklogDeletion: WorklogDeletionPresentation
    public private(set) var worklogDeletionSheetContent: WorklogDeletionPresentation
    public private(set) var canOpenWorklogDeletion: Bool

    @ObservationIgnored private var taskDailyTotals: [String: TaskDailyTotalPresentation]
    @ObservationIgnored private var taskDailyTotalModels: [String: TaskDailyTotal] = [:]

    public init(session: TrackerSession) {
        active = session.active
        worklogs = session.worklogs
        hasMoreHistory = session.hasMoreHistory
        error = session.error
        trackingError = session.trackingError
        historyUnavailable = session.historyUnavailable
        isHistoryLoading = session.isHistoryLoading
        visibleTasks = session.visibleTasks
        selectedTaskID = session.selectedTaskID
        selectedTask = session.selectedTask
        tab = session.tab
        connectionSettings = session.connectionSettings
        connectionStatusText = session.connectionStatusText
        connectionMessage = session.connectionMessage
        isChangingConnection = session.isChangingConnection
        isStale = session.isStale
        pauseOnScreenLock = session.pauseOnScreenLock
        autoPauseStatusText = session.autoPauseStatusText
        dailyTotalsStatus = session.dailyTotalsStatus
        isBlockingControls = session.isBlockingControls
        canStopTracking = session.canStopTracking
        startableTaskIDs = Self.startableTaskIDs(session)
        timerDisplayText = session.timerDisplayText
        taskCreation = session.taskCreation
        canOpenTaskCreation = session.canOpenTaskCreation
        taskRename = session.taskRename
        canOpenTaskRename = session.canOpenTaskRename
        taskArchiving = session.taskArchiving
        taskArchivingSheetContent = session.taskArchiving
        archivableTaskIDs = Self.archiveTaskIDs(session, archived: false)
        unarchivableTaskIDs = Self.archiveTaskIDs(session, archived: true)
        bulkTaskArchiving = session.bulkTaskArchiving
        bulkTaskArchivingSheetContent = session.bulkTaskArchiving
        canOpenBulkTaskArchiving = session.canOpenBulkTaskArchiving
        worklogCorrection = session.worklogCorrection
        worklogCorrectionSheetContent = session.worklogCorrection
        canOpenWorklogCorrection = session.canOpenWorklogCorrection
        worklogMove = session.worklogMove
        worklogMoveSheetContent = session.worklogMove
        canOpenWorklogMove = session.canOpenWorklogMove
        worklogDeletion = session.worklogDeletion
        worklogDeletionSheetContent = session.worklogDeletion
        canOpenWorklogDeletion = session.canOpenWorklogDeletion
        taskDailyTotals = Self.taskDailyTotals(session)
    }

    public func update(from session: TrackerSession) {
        set(\.active, session.active)
        set(\.worklogs, session.worklogs)
        set(\.hasMoreHistory, session.hasMoreHistory)
        set(\.error, session.error)
        set(\.trackingError, session.trackingError)
        set(\.historyUnavailable, session.historyUnavailable)
        set(\.isHistoryLoading, session.isHistoryLoading)
        set(\.visibleTasks, session.visibleTasks)
        set(\.selectedTaskID, session.selectedTaskID)
        set(\.selectedTask, session.selectedTask)
        set(\.tab, session.tab)
        set(\.connectionSettings, session.connectionSettings)
        set(\.connectionStatusText, session.connectionStatusText)
        set(\.connectionMessage, session.connectionMessage)
        set(\.isChangingConnection, session.isChangingConnection)
        set(\.isStale, session.isStale)
        set(\.pauseOnScreenLock, session.pauseOnScreenLock)
        set(\.autoPauseStatusText, session.autoPauseStatusText)
        set(\.dailyTotalsStatus, session.dailyTotalsStatus)
        set(\.isBlockingControls, session.isBlockingControls)
        set(\.canStopTracking, session.canStopTracking)
        set(\.startableTaskIDs, Self.startableTaskIDs(session))
        set(\.timerDisplayText, session.timerDisplayText)
        set(\.taskCreation, session.taskCreation)
        set(\.canOpenTaskCreation, session.canOpenTaskCreation)
        set(\.taskRename, session.taskRename)
        set(\.canOpenTaskRename, session.canOpenTaskRename)
        set(\.taskArchiving, session.taskArchiving)
        set(\.archivableTaskIDs, Self.archiveTaskIDs(session, archived: false))
        set(\.unarchivableTaskIDs, Self.archiveTaskIDs(session, archived: true))
        set(\.bulkTaskArchiving, session.bulkTaskArchiving)
        set(\.canOpenBulkTaskArchiving, session.canOpenBulkTaskArchiving)
        set(\.worklogCorrection, session.worklogCorrection)
        set(\.canOpenWorklogCorrection, session.canOpenWorklogCorrection)
        set(\.worklogMove, session.worklogMove)
        set(\.canOpenWorklogMove, session.canOpenWorklogMove)
        set(\.worklogDeletion, session.worklogDeletion)
        set(\.canOpenWorklogDeletion, session.canOpenWorklogDeletion)
        // SwiftUI continues rendering a sheet while its dismissal animates, so keep the last presented content.
        if taskArchiving.isPresented { set(\.taskArchivingSheetContent, taskArchiving) }
        if bulkTaskArchiving.isPresented { set(\.bulkTaskArchivingSheetContent, bulkTaskArchiving) }
        if worklogCorrection.isPresented { set(\.worklogCorrectionSheetContent, worklogCorrection) }
        if worklogMove.isPresented { set(\.worklogMoveSheetContent, worklogMove) }
        if worklogDeletion.isPresented { set(\.worklogDeletionSheetContent, worklogDeletion) }
        updateTaskDailyTotals(Self.taskDailyTotals(session))
    }

    public func canStartTracking(taskID: String) -> Bool { startableTaskIDs.contains(taskID) }
    public func canArchiveTask(taskID: String) -> Bool { archivableTaskIDs.contains(taskID) }
    public func canUnarchiveTask(taskID: String) -> Bool { unarchivableTaskIDs.contains(taskID) }

    /// Returns the task's observable total. Totals for tasks that leave the catalog stop updating.
    public func taskDailyTotal(taskID: String) -> TaskDailyTotal {
        if let existing = taskDailyTotalModels[taskID] { return existing }
        let model = TaskDailyTotal(content: taskDailyTotals[taskID])
        taskDailyTotalModels[taskID] = model
        return model
    }

    private func set<Value: Equatable>(_ keyPath: ReferenceWritableKeyPath<TrackerPresentation, Value>, _ value: Value)
    {
        guard self[keyPath: keyPath] != value else { return }
        self[keyPath: keyPath] = value
    }

    private func updateTaskDailyTotals(_ next: [String: TaskDailyTotalPresentation]) {
        taskDailyTotals = next
        for (taskID, model) in taskDailyTotalModels {
            if let content = next[taskID] {
                if model.content != content { model.content = content }
            } else {
                taskDailyTotalModels.removeValue(forKey: taskID)
            }
        }
    }

    private static func startableTaskIDs(_ session: TrackerSession) -> Set<String> {
        Set(session.tasks.map(\.id).filter { session.canStartTracking(taskID: $0) })
    }

    private static func archiveTaskIDs(_ session: TrackerSession, archived: Bool) -> Set<String> {
        let availability = session.taskArchivingAvailability
        return Set(session.tasks.filter { $0.archived == archived && availability[$0.id] == true }.map(\.id))
    }

    private static func taskDailyTotals(_ session: TrackerSession) -> [String: TaskDailyTotalPresentation] {
        let explanation = session.dailyTotalsExplanation
        return Dictionary(
            uniqueKeysWithValues: session.tasks.map {
                (
                    $0.id,
                    TaskDailyTotalPresentation(
                        text: session.dailyDuration(taskID: $0.id).map(clockDuration) ?? "-",
                        explanation: explanation)
                )
            })
    }
}
