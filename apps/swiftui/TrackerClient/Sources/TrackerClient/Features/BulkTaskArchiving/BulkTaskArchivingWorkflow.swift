import Foundation

/// Previews inactive tasks and archives a reviewed preview.
@MainActor
final class BulkTaskArchivingWorkflow: EditorWorkflow {
    let state = BulkTaskArchivingState()
    var editor: any TrackerEditor { state }
    private unowned let operations: any OperationRunner
    private unowned let resources: any WorkflowResources
    private let client: any TrackerClient
    private let clock: any TrackerClock

    init(
        operations: any OperationRunner, resources: any WorkflowResources, client: any TrackerClient,
        clock: any TrackerClock
    ) {
        self.operations = operations
        self.resources = resources
        self.client = client
        self.clock = clock
    }

    @discardableResult
    func drainWrite() -> Bool {
        guard operations.isIdle, !operations.isSleeping, let submission = state.takeSubmission() else { return false }
        operations.run(blocksControls: true) { [self] token in await archive(submission, token: token) }
        return true
    }

    /// Starts the pending preview without disabling tracking controls.
    @discardableResult
    func drainRead() -> Bool {
        guard operations.isIdle, !operations.isSleeping,
            let search = state.takeSearch(asOf: commandTimestamp(clock.now))
        else { return false }
        operations.run(blocksControls: false) { [self] token in await preview(search, token: token) }
        return true
    }

    private func preview(_ search: BulkTaskArchivingState.Search, token: Int) async {
        do {
            if resources.isStale {
                let taskList = try await client.refreshTaskList()
                guard operations.isCurrent(token), !operations.isSleeping else { return }
                resources.acceptTaskList(taskList)
            }
            guard state.matches(search) else { return }
            let currentSearch = BulkTaskArchivingState.Search(
                generation: search.generation, days: search.days, asOf: commandTimestamp(clock.now))
            let value = try await client.previewInactiveTasks(
                inactiveDays: currentSearch.days, asOf: currentSearch.asOf)
            guard operations.isCurrent(token), !operations.isSleeping else { return }
            try state.accept(value, search: currentSearch)
        } catch {
            guard operations.isCurrent(token), !operations.isSleeping else { return }
            state.failSearch(error, search: search)
            if state.matches(search), let failure = error as? BridgeFailure,
                failure.requiresRefresh || failure.uncertain || failure.kind == "unavailable"
                    || failure.kind == "protocol"
            {
                resources.recordConnectionFailure(error)
            }
        }
    }

    private func archive(_ submission: BulkTaskArchivingState.Submission, token: Int) async {
        guard !operations.isSleeping, state.beginSubmission(submission) else { return }
        do {
            let result = try await client.archiveInactiveTasks(preview: submission.preview)
            guard operations.isCurrent(token) else { return }
            guard result.archivedCount >= 0 else {
                throw BridgeFailure(
                    message: "The tracker returned an invalid archived count.", kind: "protocol",
                    requiresRefresh: true)
            }
            state.complete(count: result.archivedCount)
            await resources.refreshAfterCommit(token: token)
        } catch {
            guard operations.isCurrent(token) else { return }
            await resources.reconcileWriteFailure(token: token)
            guard operations.isCurrent(token) else { return }
            state.failSubmission(error)
        }
    }
}
