import Foundation

/// Searches destination tasks and moves reviewed worklogs.
@MainActor
final class WorklogMoveWorkflow {
    let state = WorklogMoveState()
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

    /// Opens the mover for a loaded or running worklog. Returns false when neither has the ID.
    func open(worklogID: String) -> Bool {
        let active = resources.active
        let worklog =
            state.original ?? resources.worklogs.first { $0.id == worklogID }
            ?? (active?.id == worklogID ? active : nil)
        guard let worklog else { return false }
        state.open(
            worklog, sourceTaskName: resources.tasks.first { $0.id == worklog.taskId }?.name ?? "Unknown task",
            historyPageLimit: WorklogLookup.pageLimit(loadedWorklogs: resources.worklogs.count))
        drainRead()
        return true
    }

    func reviewLatest() {
        let taskID = state.presentation.latest?.taskId
        state.reviewLatest(taskName: resources.tasks.first { $0.id == taskID }?.name)
        drainRead()
    }

    @discardableResult
    func drainWrite() -> Bool { guardedWrite.drain(self) }

    /// Starts the pending destination search without disabling tracking controls.
    @discardableResult
    func drainRead() -> Bool {
        guard operations.isIdle, !operations.isSleeping, let search = state.takeSearch() else { return false }
        operations.run(blocksControls: false) { [self] token in await loadCandidates(search, token: token) }
        return true
    }

    private func loadCandidates(_ initialSearch: WorklogMoveState.Search, token: Int) async {
        var search = initialSearch
        do {
            if search.refresh {
                guard try await resources.refreshTaskList(token: token, recordsFailure: false) != nil else { return }
                state.didRefresh(search)
            }
            if operations.isSleeping { state.deferSearch(); return }
            guard let currentSearch = state.currentSearch(replacing: search) else { return }
            search = currentSearch
            let values = try await client.moveCandidates(sourceTaskID: search.sourceTaskID, query: search.query)
            guard operations.isCurrent(token) else { return }
            if operations.isSleeping { state.deferSearch(); return }
            state.accept(values, search: search)
        } catch {
            guard operations.isCurrent(token) else { return }
            state.failSearch(error, search: search)
        }
    }

    private func finish(_ intent: WorklogMoveState.Intent) {
        state.reset()
        if resources.selectedTaskID == intent.expected.taskId {
            resources.removeFromHistory(worklogID: intent.expected.id)
        } else if resources.selectedTaskID == intent.destinationTaskID {
            resources.invalidateHistoryCursor()
        }
        resources.requestHistory()
        operations.publish()
    }
}

extension WorklogMoveState: GuardedWorklogState {}

extension WorklogMoveWorkflow: GuardedWorklogEditor {
    var changedMessage: String { "This worklog changed. Review it before moving." }
    var unconfirmedMessage: String { "The move response does not contain the moved worklog." }
    var resolvesCommittedWrite: Bool { state.mayHaveCommitted }

    // A committed move leaves the worklog in the destination task, so search it first.
    func lookup(_ intent: WorklogMoveState.Intent, includeCommitted: Bool) -> WorklogLookup {
        WorklogLookup(
            expected: intent.expected,
            taskIDs: includeCommitted ? [intent.destinationTaskID, intent.expected.taskId] : [intent.expected.taskId],
            historyPageLimit: intent.historyPageLimit, dialog: "mover")
    }

    func preflight(
        _ intent: WorklogMoveState.Intent, latest: WorklogItem?, resources: TaskListResources,
        resolveCommitted: Bool
    ) -> WorklogWritePreflight {
        if resolveCommitted, let latest, intent.matchesReplacement(latest) { return .committed }
        guard let latest else {
            return .review(
                latest: nil, message: "This worklog was moved or deleted. Cancel the mover and refresh history.")
        }
        guard latest.id == intent.expected.id, latest.taskId == intent.expected.taskId,
            sameWorklogTimestamp(latest.start, intent.expected.start),
            sameWorklogTimestamp(latest.end, intent.expected.end)
        else {
            return .review(latest: latest, message: changedMessage)
        }
        guard resources.catalog.value.contains(where: { $0.id == intent.destinationTaskID && !$0.archived }) else {
            return .reject(
                BridgeFailure(message: "The destination task is unavailable or archived. Choose another task."))
        }
        return .apply
    }

    func write(_ intent: WorklogMoveState.Intent) async throws -> WorklogItem {
        try await client.moveWorklog(expected: intent.expected, destinationTaskID: intent.destinationTaskID).worklog
    }

    func confirms(_ worklog: WorklogItem, _ intent: WorklogMoveState.Intent) -> Bool {
        intent.matchesReplacement(worklog)
    }

    func finishCommitted(_ intent: WorklogMoveState.Intent) { finish(intent) }

    func complete(_ intent: WorklogMoveState.Intent, token: Int) async {
        finish(intent)
        await resources.refreshAfterCommit(token: token)
    }
}
