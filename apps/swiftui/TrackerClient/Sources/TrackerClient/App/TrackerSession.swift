import Foundation

@MainActor
public final class TrackerSession {
    public var onChange: (() -> Void)?
    public private(set) var now: Date

    public private(set) var taskCatalogResource: TaskCatalogObservation?
    public private(set) var trackingResource: TrackingObservation?
    private let client: any TrackerClient
    private let reports: (any ReportClient)?
    private let clock: any TrackerClock
    private let scheduler: any TrackerScheduler
    private let settingsRepository: any ConnectionSettingsRepository
    private let trackingPreferencesRepository: (any TrackingPreferencesRepository)?
    private let automation: TrackingAutomationState
    private let connection: ConnectionState
    private let catalog = TaskCatalogState()
    private let tracking = TrackingState()
    private let lastTracked: LastTrackedTaskState
    private let history = WorklogHistoryState()
    private let creation = TaskCreationState()
    private let rename = TaskRenameState()
    private let archiving = TaskArchivingState()
    private let bulkArchiving = BulkTaskArchivingState()
    private let correction = WorklogCorrectionState()
    private let move = WorklogMoveState()
    private let deletion = WorklogDeletionState()
    private let dailyTotals: DailyTotalsState
    private var displayTimer: (any TrackerCancellation)?
    private var pollTimer: (any TrackerCancellation)?
    private var rolloverTimer: (any TrackerCancellation)?
    private var displayGeneration = 0
    private var pollGeneration = 0
    private var rolloverGeneration = 0
    private var started = false
    // One poll at a time: no poll is scheduled while a refresh is waiting.
    private var refreshing = false
    private var sleeping = false
    private var windowVisible = false
    private var menuOpen = false
    private var uiVisible: Bool { windowVisible || menuOpen }

    public init(
        client: any TrackerClient, clock: any TrackerClock,
        scheduler: any TrackerScheduler, settings: any ConnectionSettingsRepository,
        trackingPreferences: (any TrackingPreferencesRepository)? = nil,
        lastTrackedTasks: (any LastTrackedTaskRepository)? = nil,
        reports: (any ReportClient)? = nil, calendar: Calendar = .autoupdatingCurrent
    ) {
        self.client = client
        self.reports = reports
        dailyTotals = DailyTotalsState(calendar: calendar)
        self.clock = clock
        self.scheduler = scheduler
        settingsRepository = settings
        trackingPreferencesRepository = trackingPreferences
        automation = TrackingAutomationState(preferences: trackingPreferences?.load() ?? TrackingPreferences())
        let initialSettings = settings.load() ?? .local
        connection = ConnectionState(settings: initialSettings)
        lastTracked = LastTrackedTaskState(repository: lastTrackedTasks, settings: initialSettings)
        now = clock.now
        dailyTotals.updateDay(at: clock.now)
    }

    deinit {
        displayTimer?.cancel()
        pollTimer?.cancel()
        rolloverTimer?.cancel()
    }

    public var pauseOnScreenLock: Bool { automation.enabled }
    public var autoPauseStatusText: String? { automation.statusText }
    public var tasks: [TaskItem] { catalog.tasks }
    public var active: WorklogItem? { tracking.active }
    public var worklogs: [WorklogItem] { history.worklogs }
    public var nextCursor: String? { history.nextCursor }
    public var error: String? { history.error }
    public var trackingError: String? { tracking.error }
    public var historyUnavailable: Bool { history.unavailable }
    public var isHistoryLoading: Bool { history.isLoading }
    public var selectedTaskID: String? { catalog.selectedTaskID }
    public var tab: TaskTab { catalog.tab }
    public var connectionSettings: ConnectionSettings { connection.settings }
    public var connectionStatusText: String { connection.statusText }
    public var connectionMessage: String? { connection.message }
    public var isChangingConnection: Bool { connection.changing }
    public var isStale: Bool { connection.stale }
    public var visibleTasks: [TaskItem] { catalog.visibleTasks }
    public var selectedTask: TaskItem? { catalog.selectedTask }
    public var hasMoreHistory: Bool { nextCursor != nil }
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
    public var lastTrackedTaskID: String? { lastTracked.taskID }
    public var lastTrackedTask: TaskItem? { lastTrackedTaskID.flatMap { task(withID: $0) } }
    public var menuPrimaryAction: MenuPrimaryAction {
        guard !automation.locked, canSendTracking else { return .disabled }
        if let active { return .stop(worklogID: active.id) }
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
    public var timerDisplayText: String {
        guard connection.confirmed else { return "Unavailable" }
        return elapsed.map(clockDuration) ?? "Idle"
    }
    public var elapsed: TimeInterval? { tracking.elapsed(clock: clock) }
    public var dailyTotalsStatus: DailyTotalsStatus {
        guard let day = dailyTotals.day, clock.now >= day.start, clock.now < day.end else { return .unavailable }
        return dailyTotals.status
    }
    public var dailyTotalsError: String? { dailyTotals.error }
    public var dailyTotalsDayStart: Date? { dailyTotals.day?.start }
    public var totalDailyDuration: TimeInterval? {
        dailyTotals.totalDuration(active: active, clock: clock)
    }
    public var totalDailyDurationText: String {
        totalDailyDuration.map(clockDuration) ?? "Unavailable"
    }
    public var dailyTotalsExplanation: String {
        switch dailyTotalsStatus {
        case .current: return "Time logged today in your local time zone"
        case .cached: return "Today's total uses cached tracker state. Running time may be unconfirmed."
        case .loading: return "Loading today's totals"
        case .unavailable: return dailyTotalsError ?? "Today's total is unavailable"
        }
    }
    public var todayTasks: [TaskItem] {
        tasks.filter { (dailyDuration(taskID: $0.id) ?? 0) > 0 }
            .sorted {
                if $0.name == $1.name { return $0.id < $1.id }
                return $0.name < $1.name
            }
    }
    public func dailyDuration(taskID: String) -> TimeInterval? {
        dailyTotals.duration(taskID: taskID, active: active, clock: clock)
    }
    public func dailyDurationText(taskID: String) -> String {
        dailyDuration(taskID: taskID).map(clockDuration) ?? "Unavailable"
    }

    public func start() {
        guard !started else { return }
        started = true
        dailyTotals.updateDay(at: clock.now)
        updateRolloverTimer()
        updateDisplayTimer()
        refreshing = true
        let settings = connection.settings
        Task { [weak self] in
            guard let self else { return }
            do {
                acceptTaskList(try await client.openConfigured(settings))
                await loadReport()
            } catch {
                recordConnectionFailure(error)
            }
            finishRefresh()
        }
    }

    public func setWindowVisible(_ visible: Bool) {
        let previouslyVisible = uiVisible
        windowVisible = visible
        visibilityChanged(previouslyVisible: previouslyVisible)
    }

    public func menuOpened() {
        let previouslyVisible = uiVisible
        menuOpen = true
        visibilityChanged(previouslyVisible: previouslyVisible)
    }

    public func menuClosed() {
        let previouslyVisible = uiVisible
        menuOpen = false
        visibilityChanged(previouslyVisible: previouslyVisible)
    }

    public func sleep() {
        guard started else { return }
        sleeping = true
        cancelTimers()
        publish()
    }

    public func wake() {
        guard started else { return }
        sleeping = false
        tracking.resetElapsedAnchor(clock: clock)
        dailyTotals.updateDay(at: clock.now)
        dailyTotals.reanchor(clock: clock)
        updateDisplayTimer()
        updateRolloverTimer()
        refresh()
    }

    public var bulkTaskArchiving: BulkTaskArchivingPresentation { bulkArchiving.presentation }
    public var canOpenBulkTaskArchiving: Bool { connection.confirmed }

    public func openBulkTaskArchiving() {
        guard canOpenBulkTaskArchiving else { return }
        loadBulkArchivePreview(bulkArchiving.open(asOf: commandTimestamp(clock.now)))
        publish()
    }
    public func updateBulkArchiveDays(_ text: String) {
        loadBulkArchivePreview(bulkArchiving.updateDays(text, asOf: commandTimestamp(clock.now)))
        publish()
    }
    public func refreshBulkArchivePreview() {
        loadBulkArchivePreview(bulkArchiving.requestPreview(asOf: commandTimestamp(clock.now)))
        publish()
    }
    public func cancelBulkTaskArchiving() {
        bulkArchiving.cancel()
        publish()
    }
    public func submitBulkTaskArchiving() {
        guard let preview = bulkArchiving.submit() else { return }
        publish()
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
        publish()
    }

    public func unarchiveTask(taskID: String) {
        guard canUnarchiveTask(taskID: taskID), let task = task(withID: taskID) else { return }
        archiving.open(task, action: .unarchive)
        submitTaskArchiving()
    }

    public func cancelTaskArchiving() {
        archiving.cancel()
        publish()
    }

    public func submitTaskArchiving() {
        guard let request = archiving.submit() else { return }
        let occurredAt = commandTimestamp(clock.now)
        publish()
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
                    if request.action == .unarchive { self.catalog.rememberRestoredTask(request.taskID) }
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
        publish()
    }

    public func setTaskCreationName(_ name: String) {
        creation.updateName(name)
        publish()
    }

    public func cancelTaskCreation() {
        creation.cancel()
        publish()
    }

    public func submitTaskCreation() {
        guard let name = creation.submit() else {
            publish()
            return
        }
        let occurredAt = commandTimestamp(clock.now)
        publish()
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
        publish()
    }

    public func setTaskRenameName(_ name: String) {
        rename.updateName(name)
        publish()
    }

    public func cancelTaskRename() {
        rename.cancel()
        publish()
    }

    public func submitTaskRename() {
        guard let request = rename.submit() else {
            publish()
            return
        }
        let occurredAt = commandTimestamp(clock.now)
        publish()
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
        publish()
    }

    public func openWorklogMove(worklogID: String) {
        guard canOpenWorklogMove, let worklog = worklogForEditor(worklogID) else { return }
        loadMoveCandidates(move.open(worklog, sourceTaskName: taskName(worklog.taskId)))
        publish()
    }

    public func setWorklogMoveQuery(_ query: String) {
        loadMoveCandidates(move.updateQuery(query))
        publish()
    }

    public func retryWorklogMoveCandidates() {
        loadMoveCandidates(move.retrySearch())
        publish()
    }

    public func selectWorklogMoveDestination(taskID: String) {
        move.select(taskID)
        publish()
    }

    public func moveWorklogDestinationSelection(by offset: Int) {
        move.moveSelection(by: offset)
        publish()
    }

    public func cancelWorklogMove() {
        move.cancel()
        publish()
    }

    public func submitWorklogMove() {
        guard let request = move.submit() else { return }
        publish()
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
        publish()
    }

    public func setWorklogCorrectionEnd(_ date: Date) {
        correction.updateEnd(date)
        publish()
    }

    public func cancelWorklogCorrection() {
        correction.cancel()
        publish()
    }

    public func submitWorklogCorrection() {
        guard let request = correction.submit(at: clock.now) else {
            publish()
            return
        }
        publish()
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
        publish()
    }

    public func cancelWorklogDeletion() {
        deletion.cancel()
        publish()
    }

    public func submitWorklogDeletion() {
        guard let expected = deletion.submit() else { return }
        publish()
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
        trackingPreferencesRepository?.save(TrackingPreferences(pauseOnScreenLock: enabled))
        if enabled && automation.locked { pauseTracking(at: clock.now) }
        publish()
    }

    public func screenLocked(at date: Date) {
        guard automation.lock() else { return }
        if automation.enabled { pauseTracking(at: date) }
        publish()
    }

    public func screenUnlocked(at date: Date) {
        if let resume = automation.unlock(at: date) {
            Task { [weak self] in
                guard let self else { return }
                await resumeTracking(resume)
                await reloadTaskList()
                publish()
            }
        }
        publish()
    }

    public func changeTab(_ newTab: TaskTab) {
        guard catalog.changeTab(newTab) else { return }
        requestHistory()
        publish()
    }

    public func select(_ taskID: String?) {
        guard catalog.select(taskID) else { return }
        requestHistory()
        publish()
    }

    public func refresh() {
        connection.protocolBlocked = false
        beginRefresh()
    }

    public func retryHistory() {
        guard started else { return }
        requestHistory()
        publish()
    }

    public func testConnection(_ settings: ConnectionSettings) async throws {
        try await client.test(connection.normalized(settings))
    }

    public func connect(_ settings: ConnectionSettings) async -> Bool {
        guard !connection.changing else { return false }
        connection.changing = true
        publish()
        defer {
            connection.changing = false
            publish()
        }
        do {
            let normalized = try connection.normalized(settings)
            finishConnection(try await client.connect(normalized), settings: normalized)
        } catch {
            connection.message = "Connection unchanged. \(error.localizedDescription)"
            return false
        }
        await loadReport()
        schedulePolling()
        return true
    }

    public func startTracking(taskID: String) {
        guard canStartTracking(taskID: taskID) else { return }
        let expectedActiveID = active?.id
        let occurredAt = commandTimestamp(clock.now)
        sendTracking { client in
            _ = try await client.startTracking(
                taskID: taskID, expectedActiveID: expectedActiveID, occurredAt: occurredAt)
        }
    }

    public func stopTracking(worklogID: String) {
        guard canStopTracking, active?.id == worklogID else { return }
        let occurredAt = commandTimestamp(clock.now)
        sendTracking { client in
            _ = try await client.stopTracking(worklogID: worklogID, occurredAt: occurredAt)
        }
    }

    public func dismissTrackingError() {
        tracking.error = nil
        publish()
    }

    public func loadOlder() {
        guard !connection.stale, !history.isLoading, let taskID = selectedTaskID, let cursor = nextCursor else {
            return
        }
        loadHistory(taskID: taskID, cursor: cursor)
        publish()
    }

    private func sendTracking(_ request: @escaping @MainActor (any TrackerClient) async throws -> Void) {
        automation.cancel()
        tracking.error = nil
        tracking.isSubmitting = true
        publish()
        Task { [weak self] in
            guard let self else { return }
            do {
                try await request(client)
            } catch {
                recordTrackingFailure(error)
            }
            await reloadTaskList()
            tracking.isSubmitting = false
            publish()
        }
    }

    private func pauseTracking(at date: Date) {
        guard let worklog = active, automation.beginPause() else { return }
        tracking.error = nil
        let occurredAt = commandTimestamp(date)
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
            await reloadTaskList()
            publish()
        }
    }

    private func resumeTracking(_ resume: TrackingAutomationState.Resume) async {
        tracking.error = nil
        do {
            _ = try await client.resumeTracking(
                taskID: resume.taskID, occurredAt: commandTimestamp(resume.date))
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
                if requiresConnectionRecovery(error) { recordConnectionFailure(error) }
            }
            publish()
        }
    }

    private func writeFailure(_ error: Error) -> Error {
        guard let failure = error as? BridgeFailure, failure.uncertain else { return error }
        return BridgeFailure(
            message: "Could not confirm the change. Check the reloaded data before trying again. \(failure.message)",
            kind: failure.kind, uncertain: true, requiresRefresh: failure.requiresRefresh)
    }

    private func recordTrackingFailure(_ error: Error) {
        tracking.error = writeFailure(error).localizedDescription
    }

    private func requiresConnectionRecovery(_ error: Error) -> Bool {
        guard let failure = error as? BridgeFailure else { return false }
        if failure.requiresRefresh || failure.uncertain { return true }
        return failure.kind == "unavailable" || failure.kind == "protocol"
    }

    /// Reloads the task list and, with a report client, today's totals.
    private func reloadTaskList() async {
        dailyTotals.updateDay(at: clock.now)
        if reports == nil || !connection.confirmed || connection.stale {
            do {
                acceptTaskList(try await client.refreshTaskList(settings: connection.settings))
            } catch {
                recordConnectionFailure(error)
                return
            }
        }
        await loadReport()
    }

    private func beginRefresh() {
        guard started, !sleeping, !refreshing else { return }
        refreshing = true
        cancelPolling()
        Task { [weak self] in
            guard let self else { return }
            await reloadTaskList()
            finishRefresh()
        }
    }

    private func finishRefresh() {
        refreshing = false
        schedulePolling()
    }

    /// Publishes the write result, reloads the task list, runs `afterReload`, and publishes again.
    private func reloadAfterTaskWrite(afterReload: () -> Void = {}) async {
        publish()
        await reloadTaskList()
        afterReload()
        publish()
    }

    private func showCreatedTask(_ created: TaskItem) {
        let confirmed = task(withID: created.id) ?? created
        let changedTab = catalog.changeTab(confirmed.archived ? .archived : .active)
        let changedSelection = catalog.select(created.id)
        if changedTab || changedSelection { requestHistory() }
    }

    private func reloadAfterWorklogWrite() async {
        publish()
        await reloadTaskList()
        requestHistory()
        publish()
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
            publish()
        }
    }

    private func acceptTaskList(_ resources: TaskListResources, requestReport: Bool = true) {
        taskCatalogResource = resources.catalog
        trackingResource = resources.tracking
        dailyTotals.updateDay(at: clock.now)
        let retryFailedHistory = connection.stale && history.unavailable
        connection.acceptTaskList()
        lastTracked.observe(resources, settings: connection.settings)
        let previousActive = active
        automation.observeActive(resources.tracking.value)
        tracking.apply(resources.tracking.value, clock: clock)
        if reports != nil && requestReport { dailyTotals.invalidate() }
        let selectionChanged = catalog.apply(resources.catalog.value, previousActive: previousActive, active: active)
        if selectionChanged || retryFailedHistory {
            requestHistory()
        } else if !history.unavailable {
            history.error = nil
        }
        updateDisplayTimer()
        updateRolloverTimer()
        publish()
    }

    private func loadReport() async {
        guard let reports, let requested = dailyTotals.begin(clock: clock) else { return }
        do {
            let refreshed = try await reports.refreshTaskListWithTotals(
                settings: connection.settings, start: requested.start,
                end: requested.end, now: requested.now)
            try dailyTotals.validate(refreshed.report, tracking: refreshed.taskList.tracking)
            dailyTotals.accept(
                refreshed.report, tracking: refreshed.taskList.tracking,
                requested: requested, clock: clock)
            acceptTaskList(refreshed.taskList, requestReport: false)
        } catch {
            dailyTotals.fail(error, clock: clock)
            if requiresConnectionRecovery(error) { recordConnectionFailure(error) }
            publish()
        }
    }

    private func recordConnectionFailure(_ failure: Error) {
        connection.recordFailure(failure)
        schedulePolling()
        history.error = failure.localizedDescription
        publish()
    }

    private func requestHistory() {
        history.request(selectedTaskID: selectedTaskID)
        guard let taskID = selectedTaskID else { return }
        loadHistory(taskID: taskID, cursor: nil)
    }

    private func loadHistory(taskID: String, cursor: String?) {
        history.beginLoading()
        Task { [weak self] in
            guard let self else { return }
            do {
                let page = try await client.history(taskID: taskID, cursor: cursor)
                // Drop a page whose task is no longer selected.
                guard taskID == selectedTaskID else { return }
                history.accept(page, cursor: cursor)
            } catch {
                guard taskID == selectedTaskID else { return }
                history.fail(error)
                if let failure = error as? BridgeFailure, historyFailureRequiresRecovery(failure) {
                    recordConnectionFailure(error)
                }
            }
            publish()
        }
    }

    private func schedulePolling() {
        cancelPolling()
        guard started, !sleeping, !refreshing, !connection.protocolBlocked else { return }
        let interval = TrackerPollingPolicy.interval(visible: uiVisible, failures: connection.failures)
        let token = pollGeneration
        pollTimer = scheduler.schedule(after: interval, repeating: false, tolerance: min(5, interval * 0.2)) {
            [weak self] in
            guard let self, token == pollGeneration else { return }
            beginRefresh()
        }
    }

    private func updateDisplayTimer() {
        let needed = started && !sleeping && uiVisible && active != nil
        if !needed {
            cancelDisplay()
            return
        }
        guard displayTimer == nil else { return }
        now = clock.now
        let token = displayGeneration
        displayTimer = scheduler.schedule(after: 1, repeating: true, tolerance: 0.2) { [weak self] in
            guard let self, displayTickIsCurrent(token) else { return }
            now = clock.now
            if reports != nil, dailyTotals.updateDay(at: clock.now) {
                updateRolloverTimer()
                if !connection.protocolBlocked { refresh() }
            }
            publish()
        }
    }

    private func cancelTimers() {
        cancelPolling()
        cancelDisplay()
        rolloverGeneration += 1
        rolloverTimer?.cancel()
        rolloverTimer = nil
    }

    private func updateRolloverTimer() {
        rolloverGeneration += 1
        rolloverTimer?.cancel()
        rolloverTimer = nil
        guard reports != nil, started, !sleeping, let day = dailyTotals.day else { return }
        let token = rolloverGeneration
        rolloverTimer = scheduler.schedule(
            after: max(0.001, day.end.timeIntervalSince(clock.now)),
            repeating: false, tolerance: 0
        ) { [weak self] in
            guard let self, rolloverTickIsCurrent(token) else { return }
            dailyTotals.updateDay(at: clock.now)
            updateRolloverTimer()
            if !connection.protocolBlocked { refresh() }
            publish()
        }
    }

    private func cancelPolling() {
        pollGeneration += 1
        pollTimer?.cancel()
        pollTimer = nil
    }

    private func cancelDisplay() {
        displayGeneration += 1
        displayTimer?.cancel()
        displayTimer = nil
    }

    private func visibilityChanged(previouslyVisible: Bool) {
        updateDisplayTimer()
        if uiVisible && !previouslyVisible {
            refresh()
        } else if uiVisible != previouslyVisible {
            schedulePolling()
        }
        publish()
    }

    private func task(withID id: String) -> TaskItem? { tasks.first { $0.id == id } }

    private func taskName(_ id: String) -> String { task(withID: id)?.name ?? "Unknown task" }

    private func worklogForEditor(_ id: String) -> WorklogItem? {
        worklog(withID: id) ?? (active?.id == id ? active : nil)
    }

    private func worklog(withID id: String) -> WorklogItem? { worklogs.first { $0.id == id } }

    private func finishConnection(_ resources: TaskListResources, settings: ConnectionSettings) {
        automation.cancel()
        connection.settings = settings
        dailyTotals.clear(at: clock.now)
        settingsRepository.save(settings)
        catalog.resetSelections()
        history.request(selectedTaskID: nil)
        tracking.error = nil
        acceptTaskList(resources)
    }

    private func historyFailureRequiresRecovery(_ failure: BridgeFailure) -> Bool {
        failure.kind == "unavailable" || failure.kind == "protocol" || failure.requiresRefresh
    }

    private func displayTickIsCurrent(_ token: Int) -> Bool {
        token == displayGeneration && started && !sleeping && uiVisible && active != nil
    }

    private func rolloverTickIsCurrent(_ token: Int) -> Bool { token == rolloverGeneration && started && !sleeping }

    private func publish() { onChange?() }
}
