import Foundation

extension WorklogLookup {
    /// Reads one history page beyond the pages loaded when the editor opened, and at least two pages.
    static func pageLimit(loadedWorklogs count: Int) -> Int { max(1, (count + 49) / 50) + 1 }
}

/// What a reviewed worklog intent needs once the latest copy of the worklog is known.
enum WorklogWritePreflight {
    case apply
    /// An earlier attempt already committed the intent.
    case committed
    case review(latest: WorklogItem?, message: String)
    case reject(Error)
}

/// The editor state transitions that a guarded worklog write drives.
@MainActor
protocol GuardedWorklogState: AnyObject {
    associatedtype Intent
    func takePendingIntent() -> Intent?
    func deferUntilWake()
    /// Records that a write was sent, so a later attempt may find it committed.
    func commandStarted()
    func requireReview(latest: WorklogItem?, message: String)
    func fail(_ failure: Error, retainIntent: Bool)
}

extension GuardedWorklogState {
    func commandStarted() {}
}

/// A worklog editor that `GuardedWorklogWrite` applies after checking the reviewed worklog.
@MainActor
protocol GuardedWorklogEditor: AnyObject {
    associatedtype State: GuardedWorklogState
    var state: State { get }
    /// Shown when the worklog changed after the user reviewed it.
    var changedMessage: String { get }
    /// Shown when a write response does not confirm the intent.
    var unconfirmedMessage: String { get }
    /// Whether the first read may find the intent already committed.
    var resolvesCommittedWrite: Bool { get }
    func lookup(_ intent: State.Intent, includeCommitted: Bool) -> WorklogLookup
    func preflight(
        _ intent: State.Intent, latest: WorklogItem?, resources: TaskListResources, resolveCommitted: Bool
    ) -> WorklogWritePreflight
    /// Sends the write and returns the worklog in its response.
    func write(_ intent: State.Intent) async throws -> WorklogItem
    func confirms(_ worklog: WorklogItem, _ intent: State.Intent) -> Bool
    /// Finishes an intent that a read found already committed.
    func finishCommitted(_ intent: State.Intent)
    /// Finishes an intent after a confirmed write response.
    func complete(_ intent: State.Intent, token: Int) async
}

/// Applies a reviewed worklog intent. It refreshes the task list, reads the
/// worklog from bounded history, compares it with the reviewed copy, writes,
/// and reconciles after a failed or uncertain write.
@MainActor
struct GuardedWorklogWrite {
    unowned let operations: any OperationRunner
    unowned let resources: any WorkflowResources

    @discardableResult
    func drain<Editor: GuardedWorklogEditor>(_ editor: Editor) -> Bool {
        guard operations.isIdle, !operations.isSleeping, let intent = editor.state.takePendingIntent() else {
            return false
        }
        operations.run(blocksControls: true) { token in
            await apply(intent, editor: editor, token: token)
        }
        return true
    }

    private func apply<Editor: GuardedWorklogEditor>(
        _ intent: Editor.State.Intent, editor: Editor, token: Int
    ) async {
        let state = editor.state
        var commandStarted = false
        do {
            guard let taskList = try await resources.refreshTaskList(token: token, recordsFailure: false) else {
                return
            }
            if operations.isSleeping { state.deferUntilWake(); return }
            let latest = try await resources.findWorklog(
                editor.lookup(intent, includeCommitted: editor.resolvesCommittedWrite), in: taskList, token: token)
            guard operations.isCurrent(token) else { return }
            if operations.isSleeping { state.deferUntilWake(); return }
            let preflight = editor.preflight(
                intent, latest: latest, resources: taskList, resolveCommitted: editor.resolvesCommittedWrite)
            guard resolve(preflight, intent: intent, editor: editor) else { return }
            commandStarted = true
            state.commandStarted()
            let response = try await editor.write(intent)
            guard operations.isCurrent(token) else { return }
            guard editor.confirms(response, intent) else {
                throw BridgeFailure(
                    message: editor.unconfirmedMessage, kind: "protocol", uncertain: true, requiresRefresh: true)
            }
            await editor.complete(intent, token: token)
        } catch {
            await recover(from: error, intent: intent, editor: editor, commandStarted: commandStarted, token: token)
        }
    }

    private func recover<Editor: GuardedWorklogEditor>(
        from error: Error, intent: Editor.State.Intent, editor: Editor, commandStarted: Bool, token: Int
    ) async {
        guard operations.isCurrent(token) else { return }
        let state = editor.state
        let failure = error as? BridgeFailure
        let uncertain =
            failure == nil || failure?.uncertain == true || failure?.kind == "unavailable"
            || failure?.kind == "protocol"
        var reconciled = false
        if commandStarted {
            if let taskList = await resources.reconcileWriteFailure(token: token) {
                if operations.isSleeping { return retain(error, state: state) }
                do {
                    // A lost response can follow a committed write, so look where a committed write leaves the worklog.
                    let latest = try await resources.findWorklog(
                        editor.lookup(intent, includeCommitted: true), in: taskList, token: token)
                    guard operations.isCurrent(token) else { return }
                    if operations.isSleeping { return retain(error, state: state) }
                    reconciled = true
                    let preflight = editor.preflight(
                        intent, latest: latest, resources: taskList, resolveCommitted: uncertain)
                    guard resolve(preflight, intent: intent, editor: editor) else { return }
                    if failure?.kind == "worklog_changed" {
                        state.requireReview(latest: latest, message: editor.changedMessage)
                        operations.publish()
                        return
                    }
                } catch {
                    guard operations.isCurrent(token) else { return }
                    resources.recordConnectionFailure(error)
                }
            }
        } else if uncertain || failure?.requiresRefresh == true {
            resources.recordConnectionFailure(error)
        }
        guard operations.isCurrent(token) else { return }
        state.fail(error, retainIntent: uncertain || (commandStarted && !reconciled))
        operations.publish()
    }

    /// Keeps the intent after an uncertain write when sleep interrupts recovery.
    private func retain(_ error: Error, state: some GuardedWorklogState) {
        state.fail(error, retainIntent: true)
        operations.publish()
    }

    private func resolve<Editor: GuardedWorklogEditor>(
        _ preflight: WorklogWritePreflight, intent: Editor.State.Intent, editor: Editor
    ) -> Bool {
        switch preflight {
        case .apply: return true
        case .committed: editor.finishCommitted(intent)
        case .review(let latest, let message): editor.state.requireReview(latest: latest, message: message)
        case .reject(let failure): editor.state.fail(failure, retainIntent: false)
        }
        return false
    }
}
