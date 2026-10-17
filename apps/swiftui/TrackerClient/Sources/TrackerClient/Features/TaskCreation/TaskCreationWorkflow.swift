import Foundation

/// Creates reviewed tasks and selects the created task.
@MainActor
final class TaskCreationWorkflow: EditorWorkflow {
    let state = TaskCreationState()
    var editor: any TrackerEditor { state }
    private unowned let operations: any OperationRunner
    private unowned let resources: any WorkflowResources
    private let client: any TrackerClient

    init(operations: any OperationRunner, resources: any WorkflowResources, client: any TrackerClient) {
        self.operations = operations
        self.resources = resources
        self.client = client
    }

    @discardableResult
    func drainWrite() -> Bool {
        guard operations.isIdle, !operations.isSleeping, state.pending, let intent = state.intent else { return false }
        state.pending = false
        operations.run(blocksControls: true) { [self] token in await create(intent, token: token) }
        return true
    }

    private func create(_ intent: TaskCreationState.Intent, token: Int) async {
        var commandStarted = false
        do {
            if !resources.isConnectionConfirmed || resources.isStale {
                guard try await resources.refreshTaskList(token: token, recordsFailure: true) != nil else { return }
            }
            if operations.isSleeping {
                state.pending = true
                return
            }
            commandStarted = true
            let created = try await client.createTask(name: intent.name, occurredAt: intent.occurredAt).task
            guard operations.isCurrent(token) else { return }
            guard !created.id.isEmpty else {
                throw BridgeFailure(
                    message: "The creation response does not contain the created task.",
                    kind: "protocol", uncertain: true, requiresRefresh: true)
            }
            state.reset()
            await resources.refreshAfterCommit(token: token)
            guard operations.isCurrent(token) else { return }
            resources.showTask(resources.tasks.first { $0.id == created.id } ?? created)
        } catch {
            guard operations.isCurrent(token) else { return }
            let unresolved = TaskCreationState.requiresRecovery(error)
            if unresolved && commandStarted { await resources.reconcileWriteFailure(token: token) }
            guard operations.isCurrent(token) else { return }
            state.fail(error, retainIntent: unresolved)
            operations.publish()
        }
    }
}
