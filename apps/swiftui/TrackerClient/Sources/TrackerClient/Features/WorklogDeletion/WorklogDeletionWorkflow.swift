import Foundation

/// Opens the deletion dialog and deletes reviewed completed worklogs.
@MainActor
final class WorklogDeletionWorkflow {
    let state = WorklogDeletionState()
    private unowned let operations: any OperationRunner
    private unowned let resources: any WorkflowResources
    private let client: any TrackerClient
    private let guardedWrite: GuardedWorklogWrite

    init(operations: any OperationRunner, resources: any WorkflowResources, client: any TrackerClient) {
        self.operations = operations
        self.resources = resources
        self.client = client
        guardedWrite = GuardedWorklogWrite(operations: operations, resources: resources)
    }

    /// Opens the dialog for a loaded worklog. Returns false when history does not have the ID.
    func open(worklogID: String) -> Bool {
        guard let worklog = state.original ?? resources.worklogs.first(where: { $0.id == worklogID }) else {
            return false
        }
        state.open(
            worklog, taskName: resources.tasks.first { $0.id == worklog.taskId }?.name ?? "Unknown task",
            historyPageLimit: WorklogLookup.pageLimit(loadedWorklogs: resources.worklogs.count))
        return true
    }

    func reviewLatest() {
        let taskID = state.presentation.latest?.taskId
        state.reviewLatest(taskName: resources.tasks.first { $0.id == taskID }?.name)
    }

    @discardableResult
    func drainWrite() -> Bool { guardedWrite.drain(self) }

    private func finish(_ intent: WorklogDeletionState.Intent) {
        state.reset()
        if resources.selectedTaskID == intent.expected.taskId {
            resources.removeFromHistory(worklogID: intent.expected.id)
        }
        resources.requestHistory()
        operations.publish()
    }
}

extension WorklogDeletionState: GuardedWorklogState {}

extension WorklogDeletionWorkflow: GuardedWorklogEditor {
    var changedMessage: String { "This worklog changed. Review it before deleting." }
    var unconfirmedMessage: String { "The deletion response does not contain the deleted worklog." }
    var resolvesCommittedWrite: Bool { state.mayHaveCommitted }

    func lookup(_ intent: WorklogDeletionState.Intent, includeCommitted _: Bool) -> WorklogLookup {
        WorklogLookup(
            expected: intent.expected, taskIDs: [intent.expected.taskId],
            historyPageLimit: intent.historyPageLimit, dialog: "dialog")
    }

    func preflight(
        _ intent: WorklogDeletionState.Intent, latest: WorklogItem?, resources _: TaskListResources,
        resolveCommitted: Bool
    ) -> WorklogWritePreflight {
        guard let latest else {
            // A missing worklog after an uncertain write means the deletion committed.
            if resolveCommitted { return .committed }
            return .review(latest: nil, message: "This worklog was moved or deleted. Cancel and refresh history.")
        }
        guard intent.matches(latest) else { return .review(latest: latest, message: changedMessage) }
        return .apply
    }

    func write(_ intent: WorklogDeletionState.Intent) async throws -> WorklogItem {
        try await client.deleteWorklog(expected: intent.expected).worklog
    }

    func confirms(_ worklog: WorklogItem, _ intent: WorklogDeletionState.Intent) -> Bool {
        intent.matches(worklog)
    }

    func finishCommitted(_ intent: WorklogDeletionState.Intent) { finish(intent) }

    func complete(_ intent: WorklogDeletionState.Intent, token: Int) async {
        finish(intent)
        await resources.refreshAfterCommit(token: token)
    }
}
