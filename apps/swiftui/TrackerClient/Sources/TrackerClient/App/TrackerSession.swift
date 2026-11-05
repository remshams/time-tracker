import Foundation

public final class TrackerSession {
    /// Loads, refreshes and polls the stores, runs the timers and publishes after each change.
    public let sync: TrackerSync
    /// Views read the task list, tab and selection here; the sync and the session write them.
    public let taskCatalog = TaskCatalogStore()
    /// Views read the selected task's worklog pages and the history panel's state here; the store loads them.
    public let worklogHistory: WorklogHistoryStore
    /// Views read the connection settings and status here; the sync and the session write them.
    public let connection: ConnectionStore
    /// Views read the running worklog, the tracking error and the timer text here; the sync and the session write
    /// them.
    public let tracking = TrackingStore()
    /// Views read today's status and task totals here; the sync and the session write them.
    public let dailyTotals: DailyTotalsStore
    /// The menu bar reads the last tracked task here; the sync writes it.
    public let menuBar: MenuBarStore

    private let client: any TrackerClient
    // Tests compare the published store values with values computed from this clock.
    let clock: any TrackerClock
    private let settingsRepository: any ConnectionSettingsRepository
    private let trackingPreferencesRepository: any TrackingPreferencesRepository
    private let automation: TrackingAutomationState
    private let creation = TaskCreationState()
    private let rename = TaskRenameState()
    private let archiving = TaskArchivingState()
    private let bulkArchiving = BulkTaskArchivingState()
    private let correction = WorklogCorrectionState()
    private let move = WorklogMoveState()
    private let deletion = WorklogDeletionState()

    public init(
        client: any TrackerClient, clock: any TrackerClock,
        scheduler: any TrackerScheduler, settings: any ConnectionSettingsRepository,
        trackingPreferences: any TrackingPreferencesRepository,
        lastTrackedTasks: (any LastTrackedTaskRepository)? = nil,
        reports: (any ReportClient)? = nil, calendar: Calendar = .autoupdatingCurrent
    ) {
        self.client = client
        worklogHistory = WorklogHistoryStore(client: client)
        dailyTotals = DailyTotalsStore(calendar: calendar)
        self.clock = clock
        settingsRepository = settings
        trackingPreferencesRepository = trackingPreferences
        automation = TrackingAutomationState(preferences: trackingPreferences.load())
        let initialSettings = settings.load() ?? .local
        connection = ConnectionStore(settings: initialSettings)
        menuBar = MenuBarStore(repository: lastTrackedTasks, settings: initialSettings)
        dailyTotals.updateDay(at: clock.now)
        sync = TrackerSync(
            client: client, reports: reports, clock: clock, scheduler: scheduler, catalog: taskCatalog,
            history: worklogHistory, connection: connection, tracking: tracking, dailyTotals: dailyTotals,
            menuBar: menuBar, automation: automation)
    }

    public var pauseOnScreenLock: Bool { automation.enabled }
    public var autoPauseStatusText: String? { automation.statusText }
    public var tasks: [TaskItem] { taskCatalog.tasks }
    // The availability checks and the tests read these.
    public var active: WorklogItem? { tracking.active }
    public var trackingError: String? { tracking.recordedError }
    public var selectedTaskID: String? { taskCatalog.selectedTaskID }
    // Tests read these three.
    public var connectionSettings: ConnectionSettings { connection.settings }
    public var connectionStatusText: String { connection.statusText }
    public var isStale: Bool { connection.stale }
    public var selectedTask: TaskItem? { taskCatalog.selectedTask }
    public var taskCreation: TaskCreationPresentation { creation.presentation }
    public var canOpenTaskCreation: Bool { connection.confirmed }
    public var taskRename: TaskRenamePresentation { rename.presentation }
    public var canOpenTaskRename: Bool { connection.confirmed && selectedTask != nil }
    public var worklogCorrection: WorklogCorrectionPresentation { correction.presentation }
    public var canOpenWorklogCorrection: Bool { connection.confirmed }
    public var worklogMove: WorklogMovePresentation { move.presentation }
    public var canOpenWorklogMove: Bool { connection.confirmed }
    public var worklogDeletion: WorklogDeletionPresentation { deletion.presentation }
    public var canOpenWorklogDeletion: Bool { connection.confirmed }

    public var canStartSelectedTask: Bool {
        guard let selectedTaskID else { return false }
        return canStartTracking(taskID: selectedTaskID)
    }
    public func canStartTracking(taskID: String) -> Bool {
        guard canSendTracking, let task = task(withID: taskID) else { return false }
        return !task.archived && active?.taskId != task.id
    }
    public var canStopTracking: Bool { canSendTracking && active != nil }
    private var canSendTracking: Bool { connection.confirmed && !connection.stale && !tracking.isSubmitting }
    public var menuPrimaryAction: MenuPrimaryAction {
        guard !automation.locked, canSendTracking else { return .disabled }
        if let active { return .stop(worklogID: active.id) }
        let lastTrackedTask = menuBar.lastTrackedTaskID.flatMap { self.task(withID: $0) }
        guard let task = lastTrackedTask, !task.archived else { return .openMenu }
        return canStartTracking(taskID: task.id) ? .start(taskID: task.id) : .disabled
    }

    @discardableResult
    public func performMenuPrimaryAction() -> MenuPrimaryAction {
        let action = menuPrimaryAction
        switch action {
        case .stop(let worklogID): stopTracking(worklogID: worklogID)
        case .start(let taskID): startTracking(taskID: taskID)
        case .openMenu, .disabled: break
        }
        return action
    }

    public var runningTaskName: String {
        guard connection.confirmed else { return "Waiting for tracker state" }
        guard let active else { return "No timer running" }
        return taskName(active.taskId)
    }
    public var elapsed: TimeInterval? { tracking.elapsed(clock: clock) }
    /// Computed from the clock on each read, so copying from the menu uses the current time.
    func dailyDuration(taskID: String) -> TimeInterval? {
        dailyTotals.duration(taskID: taskID, active: active, clock: clock)
    }

    public var bulkTaskArchiving: BulkTaskArchivingPresentation { bulkArchiving.presentation }
    public var canOpenBulkTaskArchiving: Bool { connection.confirmed }

    public func openBulkTaskArchiving() {
        guard canOpenBulkTaskArchiving else { return }
        loadBulkArchivePreview(bulkArchiving.open(asOf: formatTimestamp(clock.now)))
        sync.publish()
    }
    public func updateBulkArchiveDays(_ text: String) {
        loadBulkArchivePreview(bulkArchiving.updateDays(text, asOf: formatTimestamp(clock.now)))
        sync.publish()
    }
    public func refreshBulkArchivePreview() {
        loadBulkArchivePreview(bulkArchiving.requestPreview(asOf: formatTimestamp(clock.now)))
        sync.publish()
    }
    public func cancelBulkTaskArchiving() {
        bulkArchiving.cancel()
        sync.publish()
    }
    public func submitBulkTaskArchiving() {
        guard let preview = bulkArchiving.submit() else { return }
        sync.publish()
        Task { [weak self] in
            guard let self else { return }
            do {
                let result = try await client.archiveInactiveTasks(preview: preview)
                bulkArchiving.complete(count: result.archivedCount)
            } catch {
                bulkArchiving.failSubmission(writeFailure(error))
            }
            await reloadAfterTaskWrite()
        }
    }

    public var taskArchiving: TaskArchivingPresentation { archiving.presentation }

    private var canBeginTaskArchiving: Bool { connection.confirmed && !archiving.isOpen }

    public func canArchiveTask(taskID: String) -> Bool {
        canBeginTaskArchiving && active?.taskId != taskID && task(withID: taskID)?.archived == false
    }

    public func canUnarchiveTask(taskID: String) -> Bool {
        canBeginTaskArchiving && task(withID: taskID)?.archived == true
    }

    public func openTaskArchive(taskID: String) {
        guard canArchiveTask(taskID: taskID), let task = task(withID: taskID) else { return }
        archiving.open(task, action: .archive)
        sync.publish()
    }

    public func unarchiveTask(taskID: String) {
        guard canUnarchiveTask(taskID: taskID), let task = task(withID: taskID) else { return }
        archiving.open(task, action: .unarchive)
        submitTaskArchiving()
    }

    public func cancelTaskArchiving() {
        archiving.cancel()
        sync.publish()
    }

    public func submitTaskArchiving() {
        guard let request = archiving.submit() else { return }
        let occurredAt = formatTimestamp(clock.now)
        sync.publish()
        Task { [weak self] in
            guard let self else { return }
            do {
                switch request.action {
                case .archive:
                    _ = try await client.archiveTask(
                        taskID: request.taskID, expectedName: request.taskName, occurredAt: occurredAt)
                case .unarchive:
                    _ = try await client.unarchiveTask(taskID: request.taskID, occurredAt: occurredAt)
                }
                archiving.reset()
                await reloadAfterTaskWrite {
                    if request.action == .unarchive { self.taskCatalog.rememberRestoredTask(request.taskID) }
                }
            } catch {
                archiving.fail(writeFailure(error))
                await reloadAfterTaskWrite()
            }
        }
    }

    public func openTaskCreation() {
        guard canOpenTaskCreation else { return }
        creation.open()
        sync.publish()
    }

    public func setTaskCreationName(_ name: String) {
        creation.updateName(name)
        sync.publish()
    }

    public func cancelTaskCreation() {
        creation.cancel()
        sync.publish()
    }

    public func submitTaskCreation() {
        guard let name = creation.submit() else {
            sync.publish()
            return
        }
        let occurredAt = formatTimestamp(clock.now)
        sync.publish()
        Task { [weak self] in
            guard let self else { return }
            do {
                let created = try await client.createTask(name: name, occurredAt: occurredAt).task
                creation.reset()
                await reloadAfterTaskWrite { self.showCreatedTask(created) }
            } catch {
                creation.fail(writeFailure(error))
                await reloadAfterTaskWrite()
            }
        }
    }

    public func openTaskRename(taskID: String? = nil) {
        guard canOpenTaskRename else { return }
        let target = taskID.flatMap { task(withID: $0) } ?? (taskID == nil ? selectedTask : nil)
        rename.open(target)
        sync.publish()
    }

    public func setTaskRenameName(_ name: String) {
        rename.updateName(name)
        sync.publish()
    }

    public func cancelTaskRename() {
        rename.cancel()
        sync.publish()
    }

    public func submitTaskRename() {
        guard let request = rename.submit() else {
            sync.publish()
            return
        }
        let occurredAt = formatTimestamp(clock.now)
        sync.publish()
        Task { [weak self] in
            guard let self else { return }
            do {
                _ = try await client.renameTask(
                    taskID: request.taskID, expectedName: request.originalName, name: request.name,
                    occurredAt: occurredAt)
                rename.reset()
            } catch {
                rename.fail(writeFailure(error))
            }
            await reloadAfterTaskWrite()
        }
    }

    public func openWorklogCorrection(worklogID: String) {
        guard canOpenWorklogCorrection, let worklog = worklogForEditor(worklogID) else { return }
        correction.open(worklog, taskName: taskName(worklog.taskId))
        sync.publish()
    }

    public func openWorklogMove(worklogID: String) {
        guard canOpenWorklogMove, let worklog = worklogForEditor(worklogID) else { return }
        loadMoveCandidates(move.open(worklog, sourceTaskName: taskName(worklog.taskId)))
        sync.publish()
    }

    public func setWorklogMoveQuery(_ query: String) {
        loadMoveCandidates(move.updateQuery(query))
        sync.publish()
    }

    public func retryWorklogMoveCandidates() {
        loadMoveCandidates(move.retrySearch())
        sync.publish()
    }

    public func selectWorklogMoveDestination(taskID: String) {
        move.select(taskID)
        sync.publish()
    }

    public func moveWorklogDestinationSelection(by offset: Int) {
        move.moveSelection(by: offset)
        sync.publish()
    }

    public func cancelWorklogMove() {
        move.cancel()
        sync.publish()
    }

    public func submitWorklogMove() {
        guard let request = move.submit() else { return }
        sync.publish()
        Task { [weak self] in
            guard let self else { return }
            do {
                _ = try await client.moveWorklog(
                    expected: request.expected, destinationTaskID: request.destinationTaskID)
                move.reset()
            } catch {
                move.fail(writeFailure(error))
            }
            await reloadAfterWorklogWrite()
        }
    }

    public func setWorklogCorrectionStart(_ date: Date) {
        correction.updateStart(date)
        sync.publish()
    }

    public func setWorklogCorrectionEnd(_ date: Date) {
        correction.updateEnd(date)
        sync.publish()
    }

    public func cancelWorklogCorrection() {
        correction.cancel()
        sync.publish()
    }

    public func submitWorklogCorrection() {
        guard let request = correction.submit(at: clock.now) else {
            sync.publish()
            return
        }
        sync.publish()
        Task { [weak self] in
            guard let self else { return }
            do {
                _ = try await client.correctWorklog(
                    expected: request.expected, replacementStart: request.replacementStart,
                    replacementEnd: request.replacementEnd, occurredAt: request.occurredAt)
                correction.reset()
            } catch {
                correction.fail(writeFailure(error))
            }
            await reloadAfterWorklogWrite()
        }
    }

    public func openWorklogDeletion(worklogID: String) {
        guard canOpenWorklogDeletion, let worklog = worklog(withID: worklogID) else { return }
        deletion.open(worklog, taskName: taskName(worklog.taskId))
        sync.publish()
    }

    public func cancelWorklogDeletion() {
        deletion.cancel()
        sync.publish()
    }

    public func submitWorklogDeletion() {
        guard let expected = deletion.submit() else { return }
        sync.publish()
        Task { [weak self] in
            guard let self else { return }
            do {
                _ = try await client.deleteWorklog(expected: expected)
                deletion.reset()
            } catch {
                deletion.fail(writeFailure(error))
            }
            await reloadAfterWorklogWrite()
        }
    }

    public func setPauseOnScreenLock(_ enabled: Bool) {
        guard automation.setEnabled(enabled) else { return }
        trackingPreferencesRepository.save(TrackingPreferences(pauseOnScreenLock: enabled))
        if enabled && automation.locked { pauseTracking(at: clock.now) }
        sync.publish()
    }

    public func screenLocked(at date: Date) {
        guard automation.lock() else { return }
        if automation.enabled { pauseTracking(at: date) }
        sync.publish()
    }

    public func screenUnlocked(at date: Date) {
        if let resume = automation.unlock(at: date) {
            Task { [weak self] in
                guard let self else { return }
                await resumeTracking(resume)
                await sync.reloadTaskList()
                sync.publish()
            }
        }
        sync.publish()
    }

    public func changeTab(_ newTab: TaskTab) {
        guard taskCatalog.changeTab(newTab) else { return }
        sync.reloadHistory()
        sync.publish()
    }

    public func select(_ taskID: String?) {
        guard taskCatalog.select(taskID) else { return }
        sync.reloadHistory()
        sync.publish()
    }

    public func testConnection(_ settings: ConnectionSettings) async throws {
        try await client.test(connection.normalized(settings))
    }

    public func connect(_ settings: ConnectionSettings) async -> Bool {
        guard !connection.changing else { return false }
        connection.beginChange()
        sync.publish()
        defer {
            connection.endChange()
            sync.publish()
        }
        do {
            let normalized = try connection.normalized(settings)
            finishConnection(try await client.connect(normalized), settings: normalized)
        } catch {
            connection.rejectChange(error)
            return false
        }
        await sync.loadReport()
        sync.schedulePolling()
        return true
    }

    public func startTracking(taskID: String) {
        guard canStartTracking(taskID: taskID) else { return }
        let expectedActiveID = active?.id
        let occurredAt = formatTimestamp(clock.now)
        sendTracking { client in
            _ = try await client.startTracking(
                taskID: taskID, expectedActiveID: expectedActiveID, occurredAt: occurredAt)
        }
    }

    public func stopTracking(worklogID: String) {
        guard canStopTracking, active?.id == worklogID else { return }
        let occurredAt = formatTimestamp(clock.now)
        sendTracking { client in
            _ = try await client.stopTracking(worklogID: worklogID, occurredAt: occurredAt)
        }
    }

    public func dismissTrackingError() {
        tracking.clearError()
        sync.publish()
    }

    private func sendTracking(_ request: @escaping @MainActor (any TrackerClient) async throws -> Void) {
        automation.cancel()
        tracking.clearError()
        tracking.beginSubmitting()
        sync.publish()
        Task { [weak self] in
            guard let self else { return }
            do {
                try await request(client)
            } catch {
                recordTrackingFailure(error)
            }
            await sync.reloadTaskList()
            tracking.endSubmitting()
            sync.publish()
        }
    }

    private func pauseTracking(at date: Date) {
        guard let worklog = active, automation.beginPause() else { return }
        tracking.clearError()
        let occurredAt = formatTimestamp(date)
        Task { [weak self] in
            guard let self else { return }
            do {
                let result = try await client.pauseTracking(worklogID: worklog.id, occurredAt: occurredAt)
                if let resume = automation.paused(taskID: worklog.taskId, didStop: result.didStop) {
                    await resumeTracking(resume)
                }
            } catch {
                automation.pauseFailed()
                recordTrackingFailure(error)
            }
            await sync.reloadTaskList()
            sync.publish()
        }
    }

    /// Only follows a successful pause, which already cleared the tracking error.
    private func resumeTracking(_ resume: TrackingAutomationState.Resume) async {
        do {
            _ = try await client.resumeTracking(
                taskID: resume.taskID, occurredAt: formatTimestamp(resume.date))
        } catch {
            recordTrackingFailure(error)
        }
    }

    private func loadBulkArchivePreview(_ search: BulkTaskArchivingState.Search?) {
        guard let search else { return }
        Task { [weak self] in
            guard let self else { return }
            do {
                let value = try await client.previewInactiveTasks(inactiveDays: search.days, asOf: search.asOf)
                try bulkArchiving.accept(value, search: search)
            } catch {
                guard bulkArchiving.matches(search) else { return }
                bulkArchiving.failSearch(error, search: search)
                if sync.requiresConnectionRecovery(error) { sync.recordConnectionFailure(error) }
            }
            sync.publish()
        }
    }

    private func writeFailure(_ error: Error) -> Error {
        guard let failure = error as? BridgeFailure, failure.uncertain else { return error }
        return BridgeFailure(
            message: "Could not confirm the change. Check the reloaded data before trying again. \(failure.message)",
            kind: failure.kind, uncertain: true, requiresRefresh: failure.requiresRefresh)
    }

    private func recordTrackingFailure(_ error: Error) {
        tracking.recordError(writeFailure(error).localizedDescription)
    }

    /// Publishes the write result, reloads the task list, runs `afterReload`, and publishes again.
    private func reloadAfterTaskWrite(afterReload: () -> Void = {}) async {
        sync.publish()
        await sync.reloadTaskList()
        afterReload()
        sync.publish()
    }

    private func showCreatedTask(_ created: TaskItem) {
        let confirmed = task(withID: created.id) ?? created
        let changedTab = taskCatalog.changeTab(confirmed.archived ? .archived : .active)
        let changedSelection = taskCatalog.select(created.id)
        if changedTab || changedSelection { sync.reloadHistory() }
    }

    private func reloadAfterWorklogWrite() async {
        sync.publish()
        await sync.reloadTaskList()
        sync.reloadHistory()
        sync.publish()
    }

    private func loadMoveCandidates(_ search: WorklogMoveState.Search?) {
        guard let search else { return }
        Task { [weak self] in
            guard let self else { return }
            do {
                move.accept(
                    try await client.moveCandidates(sourceTaskID: search.sourceTaskID, query: search.query),
                    search: search)
            } catch {
                move.failSearch(error, search: search)
            }
            sync.publish()
        }
    }

    private func task(withID id: String) -> TaskItem? { tasks.first { $0.id == id } }

    private func taskName(_ id: String) -> String { task(withID: id)?.name ?? "Unknown task" }

    private func worklogForEditor(_ id: String) -> WorklogItem? {
        worklog(withID: id) ?? (active?.id == id ? active : nil)
    }

    private func worklog(withID id: String) -> WorklogItem? { worklogHistory.worklogs.first { $0.id == id } }

    private func finishConnection(_ resources: TaskListResources, settings: ConnectionSettings) {
        automation.cancel()
        connection.apply(settings)
        dailyTotals.clear(at: clock.now)
        settingsRepository.save(settings)
        taskCatalog.resetSelections()
        worklogHistory.request(selectedTaskID: nil)
        tracking.clearError()
        sync.acceptTaskList(resources)
    }
}

public extension TrackerMenuSources {
    /// Reads the menu's stores from the session and the values that combine features from `presentation`.
    init(session: TrackerSession, presentation: TrackerPresentation) {
        self.init(
            catalog: session.taskCatalog, connection: session.connection, tracking: session.tracking,
            dailyTotals: session.dailyTotals, menuBar: session.menuBar, presentation: presentation)
    }
}
