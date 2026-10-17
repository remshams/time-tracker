import Foundation

/// Where to look for the latest copy of a reviewed worklog.
struct WorklogLookup {
    let expected: WorklogItem
    /// Tasks whose history can hold the worklog, searched in order.
    let taskIDs: [String]
    let historyPageLimit: Int
    /// Names the dialog in the message shown when the history limit is reached.
    let dialog: String
}

/// The shared tracker state that feature workflows read and update.
@MainActor
protocol WorkflowResources: AnyObject {
    var tasks: [TaskItem] { get }
    var active: WorklogItem? { get }
    var selectedTaskID: String? { get }
    /// The loaded history of the selected task.
    var worklogs: [WorklogItem] { get }
    var isConnectionConfirmed: Bool { get }
    var isStale: Bool { get }
    var isWindowVisible: Bool { get }

    func acceptTaskList(_ resources: TaskListResources)
    /// Refreshes and accepts the task list. Returns nil when `token` is no longer current.
    func refreshTaskList(token: Int, recordsFailure: Bool) async throws -> TaskListResources?
    func refreshAfterCommit(token: Int) async
    /// Marks the connection stale and refreshes it after a write failed.
    /// Returns nil when the refresh failed or `token` is no longer current.
    @discardableResult
    func reconcileWriteFailure(token: Int) async -> TaskListResources?
    func recordConnectionFailure(_ failure: Error)
    /// Reads at most the bounded history pages of each task in `lookup`.
    /// Returns nil when the worklog is absent or the read became stale or asleep.
    func findWorklog(_ lookup: WorklogLookup, in resources: TaskListResources, token: Int) async throws
        -> WorklogItem?

    func requestHistory()
    func clearHistory()
    func removeFromHistory(worklogID: String)
    func invalidateHistoryCursor()
    /// Shows the task's tab and selects it.
    func showTask(_ task: TaskItem)
    func rememberRestoredTask(_ taskID: String)
    func setTrackingError(_ message: String?)
}
