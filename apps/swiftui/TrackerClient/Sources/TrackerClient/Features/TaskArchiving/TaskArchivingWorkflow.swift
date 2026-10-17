import Foundation

/// Archives or restores one reviewed task.
@MainActor
final class TaskArchivingWorkflow: EditorWorkflow {
    let state = TaskArchivingState()
    var editor: any TrackerEditor { state }
    private unowned let operations: any OperationRunner
    private unowned let resources: any WorkflowResources
    private let client: any TrackerClient

    init(operations: any OperationRunner, resources: any WorkflowResources, client: any TrackerClient) {
        self.operations = operations
        self.resources = resources
        self.client = client
    }

    func reviewLatest() {
        guard let taskID = state.original?.id else { return }
        state.reviewLatest(resources.tasks.first { $0.id == taskID }, active: resources.active)
    }

    @discardableResult
    func drainWrite() -> Bool {
        guard operations.isIdle, !operations.isSleeping, let intent = state.takePendingIntent() else { return false }
        operations.run(blocksControls: true) { [self] token in await archive(intent, token: token) }
        return true
    }

    private func archive(_ intent: TaskArchivingState.Intent, token: Int) async {
        var commandStarted = false
        do {
            guard let taskList = try await resources.refreshTaskList(token: token, recordsFailure: true) else {
                return
            }
            guard settle(state.preflight(intent, resources: taskList), intent: intent) else { return }
            if operations.isSleeping { state.deferUntilWake(); return }
            commandStarted = true
            let result: TaskCommandResult
            switch intent.action {
            case .archive:
                result = try await client.archiveTask(
                    taskID: intent.taskID, expectedName: intent.taskName, occurredAt: intent.occurredAt)
            case .unarchive:
                result = try await client.unarchiveTask(taskID: intent.taskID, occurredAt: intent.occurredAt)
            }
            guard operations.isCurrent(token) else { return }
            guard result.task.id == intent.taskID, result.task.archived == intent.action.desiredArchived else {
                throw BridgeFailure(
                    message: "The response does not confirm the task's archive state.",
                    kind: "protocol", uncertain: true, requiresRefresh: true)
            }
            state.reset()
            await resources.refreshAfterCommit(token: token)
            guard operations.isCurrent(token) else { return }
            if intent.action == .unarchive { resources.rememberRestoredTask(intent.taskID) }
        } catch {
            guard operations.isCurrent(token) else { return }
            let confirmed = commandStarted ? await resources.reconcileWriteFailure(token: token) : nil
            guard operations.isCurrent(token) else { return }
            if let failure = error as? BridgeFailure, failure.kind == "conflict" {
                state.requireReview(
                    "The task changed on another client. Review its current state before continuing.",
                    latest: confirmed?.catalog.value.first { $0.id == intent.taskID })
            } else {
                if let confirmed, !settle(state.preflight(intent, resources: confirmed), intent: intent) { return }
                state.fail(
                    error,
                    retainIntent: TaskArchivingState.requiresRecovery(error) || (commandStarted && confirmed == nil))
            }
            state.setStatusVisibility(resources.isWindowVisible)
            operations.publish()
        }
    }

    /// Finishes an intent the task list already satisfies, or shows a required review.
    /// Returns true when the write still has to be sent.
    private func settle(_ preflight: TaskArchivingState.Preflight, intent: TaskArchivingState.Intent) -> Bool {
        switch preflight {
        case .apply:
            return true
        case .applied:
            if intent.action == .unarchive { resources.rememberRestoredTask(intent.taskID) }
            state.reset()
        case .review:
            state.setStatusVisibility(resources.isWindowVisible)
        }
        return false
    }
}
