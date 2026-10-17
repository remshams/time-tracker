import Foundation

/// Pauses tracking while the screen is locked and resumes it after unlock.
@MainActor
final class TrackingAutomationWorkflow {
    let state: TrackingAutomationState
    /// The task of a start this client sent but has not confirmed. A lock pause must cover it.
    var ownStartTaskID: String?
    private unowned let operations: any OperationRunner
    private unowned let resources: any WorkflowResources
    private let client: any TrackerClient

    init(
        preferences: TrackingPreferences, operations: any OperationRunner, resources: any WorkflowResources,
        client: any TrackerClient
    ) {
        state = TrackingAutomationState(preferences: preferences)
        self.operations = operations
        self.resources = resources
        self.client = client
    }

    func setEnabled(_ enabled: Bool, at date: Date) -> Bool {
        state.setEnabled(
            enabled, at: date, active: resources.active, ownStartTaskID: ownStartTaskID,
            confirmed: resources.isConnectionConfirmed)
    }

    func lock(at date: Date) {
        state.lock(
            at: date, active: resources.active, ownStartTaskID: ownStartTaskID,
            confirmed: resources.isConnectionConfirmed)
    }

    /// Starts a pending pause, or a resume after unlock. Pauses also run while the computer sleeps.
    @discardableResult
    func drain() -> Bool {
        guard operations.isIdle, resources.isConnectionConfirmed, state.enabled else { return false }
        let action: TrackingAutomationState.Pause?
        let resumeAt: Date?
        let taskID: String?
        if let pause = state.pendingPause {
            state.pendingPause = nil
            action = pause
            resumeAt = nil
            taskID = nil
        } else if !operations.isSleeping, !state.locked, let date = state.resumeAt,
            let pausedTaskID = state.pausedTaskID
        {
            state.resumeAt = nil
            action = nil
            resumeAt = date
            taskID = pausedTaskID
        } else {
            return false
        }
        let automationToken = state.generation
        resources.setTrackingError(nil)
        operations.run(blocksControls: true) { [self] token in
            do {
                // Reconcile before an automatic write. A second client may have
                // changed tracking while this request waited behind another one.
                guard let taskList = try await resources.refreshTaskList(token: token, recordsFailure: false) else {
                    return
                }
                guard isCurrent(automationToken) else { return }
                if let action {
                    guard let worklog = taskList.tracking.value, pauseTargets(action, worklog: worklog) else { return }
                    guard let start = timestamp(worklog.start) else {
                        resources.setTrackingError("Cannot pause tracking because its start time is invalid.")
                        return
                    }
                    guard pauseTimeIsValid(start, occurredAt: action.occurredAt) else {
                        resources.setTrackingError("Cannot pause tracking before the worklog start time.")
                        return
                    }
                    let result = try await client.pauseTracking(
                        worklogID: worklog.id, occurredAt: commandTimestamp(action.occurredAt))
                    guard operations.isCurrent(token) else { return }
                    await resources.refreshAfterCommit(token: token)
                    guard operations.isCurrent(token), isCurrent(automationToken) else { return }
                    if canRetainPausedTask(result) {
                        state.pausedTaskID = worklog.taskId
                    } else {
                        state.cancel()
                    }
                } else if let taskID, let resumeAt {
                    guard prepareResume(taskList, taskID: taskID) else { return }
                    guard !state.locked, !operations.isSleeping else { return }
                    state.pausedTaskID = nil
                    ownStartTaskID = taskID
                    defer { ownStartTaskID = nil }
                    let resumed = try await client.resumeTracking(
                        taskID: taskID, occurredAt: commandTimestamp(resumeAt))
                    guard operations.isCurrent(token) else { return }
                    state.acknowledgeOwnStart(resumed.active, taskID: taskID)
                    await resources.refreshAfterCommit(token: token)
                }
            } catch {
                guard operations.isCurrent(token) else { return }
                if automationToken == state.generation { state.cancel() }
                let message = error.localizedDescription
                await resources.reconcileWriteFailure(token: token)
                guard operations.isCurrent(token) else { return }
                resources.setTrackingError(message)
            }
            operations.publish()
        }
        return true
    }

    private func isCurrent(_ automationToken: Int) -> Bool { automationToken == state.generation && state.enabled }

    private func pauseTargets(_ action: TrackingAutomationState.Pause, worklog: WorklogItem) -> Bool {
        action.discoverAtStartup || action.expectedWorklogID == worklog.id
    }

    private func pauseTimeIsValid(_ start: Date, occurredAt: Date) -> Bool { start <= occurredAt }

    private func canRetainPausedTask(_ result: TrackingCommandResult) -> Bool {
        result.didStop && resources.active == nil && !resources.isStale
    }

    private func prepareResume(_ taskList: TaskListResources, taskID: String) -> Bool {
        guard taskList.tracking.value == nil,
            taskList.catalog.value.contains(where: { $0.id == taskID && !$0.archived })
        else {
            state.cancel()
            return false
        }
        return true
    }
}
