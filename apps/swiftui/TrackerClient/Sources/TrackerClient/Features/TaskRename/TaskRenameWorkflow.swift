import Foundation

/// Opens the rename editor and writes reviewed task names.
@MainActor
final class TaskRenameWorkflow: EditorWorkflow {
    let state = TaskRenameState()
    var editor: any TrackerEditor { state }
    private unowned let operations: any OperationRunner
    private unowned let resources: any WorkflowResources
    private let client: any TrackerClient

    init(operations: any OperationRunner, resources: any WorkflowResources, client: any TrackerClient) {
        self.operations = operations
        self.resources = resources
        self.client = client
    }

    /// Opens the editor for `taskID`, or for the selected task when `taskID` is nil.
    func open(taskID: String?) {
        let tasks = resources.tasks
        let selectedTaskID = resources.selectedTaskID
        let target =
            taskID.flatMap { id in tasks.first { $0.id == id } }
            ?? (taskID == nil ? tasks.first { $0.id == selectedTaskID } : nil)
        state.open(target)
    }

    @discardableResult
    func drainWrite() -> Bool {
        guard operations.isIdle, !operations.isSleeping, let intent = state.takePendingIntent() else { return false }
        operations.run(blocksControls: true) { [self] token in await rename(intent, token: token) }
        return true
    }

    private func rename(_ intent: TaskRenameState.Intent, token: Int) async {
        var commandStarted = false
        do {
            guard let taskList = try await resources.refreshTaskList(token: token, recordsFailure: true) else {
                return
            }
            guard canApply(intent, resources: taskList) else { return }
            if operations.isSleeping {
                state.deferUntilWake()
                return
            }
            commandStarted = true
            let result = try await client.renameTask(
                taskID: intent.taskID, expectedName: intent.originalName, name: intent.name,
                occurredAt: intent.occurredAt)
            guard operations.isCurrent(token) else { return }
            guard result.task.id == intent.taskID, result.task.name == intent.desiredName else {
                throw BridgeFailure(
                    message: "The rename response does not contain the updated task.",
                    kind: "protocol", uncertain: true, requiresRefresh: true)
            }
            state.reset()
            await resources.refreshAfterCommit(token: token)
        } catch {
            guard operations.isCurrent(token) else { return }
            let uncertain = TaskNameEditingPolicy.requiresRecovery(error)
            let confirmed = commandStarted ? await resources.reconcileWriteFailure(token: token) : nil
            guard operations.isCurrent(token) else { return }
            if let failure = error as? BridgeFailure, failure.kind == "conflict" {
                state.requireReview(
                    "The task changed on another client. Cancel and reopen the editor to review its current state.")
            } else {
                if let confirmed, !canApply(intent, resources: confirmed) { return }
                state.fail(error, retainIntent: uncertain || (commandStarted && confirmed == nil))
            }
            operations.publish()
        }
    }

    private func canApply(_ intent: TaskRenameState.Intent, resources: TaskListResources) -> Bool {
        guard let task = resources.catalog.value.first(where: { $0.id == intent.taskID }) else {
            return requireReview("The task no longer exists. Cancel the editor and refresh the task list.")
        }
        if task.name == intent.desiredName {
            state.reset()
            return false
        }
        guard task.name == intent.originalName else {
            return requireReview(
                "The task name changed on another client. Cancel and reopen the editor to review its current name.")
        }
        return true
    }

    private func requireReview(_ message: String) -> Bool {
        state.requireReview(message)
        return false
    }
}
