import Foundation

/// Opens the worklog editor and writes reviewed corrections.
@MainActor
final class WorklogCorrectionWorkflow {
    let state = WorklogCorrectionState()
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

    /// Opens the editor for a loaded or running worklog. Returns false when neither has the ID.
    func open(worklogID: String) -> Bool {
        let active = resources.active
        let worklog =
            state.original ?? resources.worklogs.first { $0.id == worklogID }
            ?? (active?.id == worklogID ? active : nil)
        guard let worklog else { return false }
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

    private func updateHistory(_ intent: WorklogCorrectionState.Intent) {
        if resources.selectedTaskID == intent.expected.taskId { resources.clearHistory() }
        resources.requestHistory()
    }
}

extension WorklogCorrectionState: GuardedWorklogState {}

extension WorklogCorrectionWorkflow: GuardedWorklogEditor {
    var changedMessage: String { "This worklog changed. Review its latest times before saving." }
    var unconfirmedMessage: String { "The correction response does not contain the updated worklog." }
    // A worklog that already shows the replacement times needs no second write.
    var resolvesCommittedWrite: Bool { true }

    func lookup(_ intent: WorklogCorrectionState.Intent, includeCommitted _: Bool) -> WorklogLookup {
        WorklogLookup(
            expected: intent.expected, taskIDs: [intent.expected.taskId],
            historyPageLimit: intent.historyPageLimit, dialog: "editor")
    }

    func preflight(
        _ intent: WorklogCorrectionState.Intent, latest: WorklogItem?, resources _: TaskListResources,
        resolveCommitted: Bool
    ) -> WorklogWritePreflight {
        guard let latest else {
            return .review(
                latest: nil, message: "This worklog was moved or deleted. Cancel the editor and refresh history.")
        }
        if resolveCommitted && intent.matchesReplacement(latest) { return .committed }
        guard latest == intent.expected else { return .review(latest: latest, message: changedMessage) }
        return .apply
    }

    func write(_ intent: WorklogCorrectionState.Intent) async throws -> WorklogItem {
        try await client.correctWorklog(
            expected: intent.expected, replacementStart: intent.replacementStart,
            replacementEnd: intent.replacementEnd, occurredAt: intent.occurredAt
        ).worklog
    }

    func confirms(_ worklog: WorklogItem, _ intent: WorklogCorrectionState.Intent) -> Bool {
        intent.matchesReplacement(worklog)
    }

    func finishCommitted(_ intent: WorklogCorrectionState.Intent) {
        state.reset()
        updateHistory(intent)
    }

    func complete(_ intent: WorklogCorrectionState.Intent, token: Int) async {
        state.reset()
        await resources.refreshAfterCommit(token: token)
        guard operations.isCurrent(token) else { return }
        updateHistory(intent)
    }
}
