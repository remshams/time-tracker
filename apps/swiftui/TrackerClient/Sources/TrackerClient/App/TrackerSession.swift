import Foundation

@MainActor
public final class TrackerSession {
    public var onChange: (() -> Void)?
    public private(set) var isBusy = true
    // Background reads still serialize requests without disabling controls.
    public var isBlockingControls: Bool {
        (isBusy && operationBlocksControls) || pendingControlCommand != nil || bulkArchiving.ownsPresentation
            || rename.hasPendingIntent || creation.pending || correction.pending || move.pending || archiving.pending
    }
    public private(set) var now: Date

    private enum Lifecycle { case idle, running, stopped }
    private var lifecycle: Lifecycle = .idle
    public private(set) var taskCatalogResource: TaskCatalogObservation?
    public private(set) var trackingResource: TrackingObservation?
    public private(set) var reportResource: TrackerReport?
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
    private let dailyTotals: DailyTotalsState
    private var displayTimer: (any TrackerCancellation)?
    private var pollTimer: (any TrackerCancellation)?
    private var rolloverTimer: (any TrackerCancellation)?
    private var displayGeneration = 0
    private var pollGeneration = 0
    private var rolloverGeneration = 0
    private var generation = 0
    private var pendingRefresh = false
    private var operationBlocksControls = true
    private var operationHasResources = false
    private var controlGeneration = 0
    private enum TrackingCommand {
        case start(taskID: String, expectedActiveID: String?, occurredAt: String)
        case stop(worklogID: String, occurredAt: String)
    }
    private struct ControlOperation: Sendable {
        let token: Int
        let controlGeneration: Int
    }
    private enum ControlCommand {
        case tracking(TrackingCommand, queued: Bool)
        case access(id: UUID, continuation: CheckedContinuation<ControlOperation, Error>)
        case history(taskID: String, cursor: String, historyToken: Int)
    }
    private var pendingControlCommand: ControlCommand?
    private var pendingOwnStartTaskID: String?
    private var sleeping = false
    private var windowVisible = false
    private var menuDepth = 0
    private var uiVisible: Bool { windowVisible || menuDepth > 0 }
    private var running: Bool { lifecycle == .running }

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
    public var canOpenTaskCreation: Bool {
        running && connection.confirmed && !connection.changing && !rename.presentation.isPresented
            && !rename.blocksConnectionChange && !correction.blocksConnectionChange && !move.blocksConnectionChange
            && !archiving.ownsPresentation && !bulkArchiving.ownsPresentation
    }
    public var taskRename: TaskRenamePresentation { rename.presentation }
    public var canOpenTaskRename: Bool {
        running && connection.confirmed && !connection.changing && !creation.presentation.isPresented
            && !creation.blocksConnectionChange && !correction.blocksConnectionChange && !move.blocksConnectionChange
            && !archiving.ownsPresentation && !bulkArchiving.ownsPresentation
            && (selectedTask != nil || rename.intent != nil)
    }
    public var worklogCorrection: WorklogCorrectionPresentation { correction.presentation }
    public var canOpenWorklogCorrection: Bool {
        running && connection.confirmed && !connection.changing && !creation.presentation.isPresented
            && !creation.blocksConnectionChange && !rename.presentation.isPresented && !rename.blocksConnectionChange
            && !move.blocksConnectionChange && !archiving.ownsPresentation && !bulkArchiving.ownsPresentation
    }

    public var worklogMove: WorklogMovePresentation { move.presentation }
    public var canOpenWorklogMove: Bool {
        running && connection.confirmed && !connection.changing && !creation.presentation.isPresented
            && !creation.blocksConnectionChange && !rename.presentation.isPresented && !rename.blocksConnectionChange
            && !correction.blocksConnectionChange && !archiving.ownsPresentation && !bulkArchiving.ownsPresentation
    }

    public var canStartSelectedTask: Bool {
        guard let selectedTaskID else { return false }
        return canStartTracking(taskID: selectedTaskID)
    }
    public func canStartTracking(taskID: String) -> Bool {
        guard running, !sleeping, !isBlockingControls, !isStale, connection.confirmed,
            let task = task(withID: taskID, in: tasks)
        else { return false }
        return !task.archived && active?.taskId != task.id
    }
    public var canStopTracking: Bool {
        running && !sleeping && !isBlockingControls && !isStale && connection.confirmed && active != nil
    }
    public var lastTrackedTaskID: String? { lastTracked.taskID }
    public var lastTrackedTask: TaskItem? { tasks.first { $0.id == lastTrackedTaskID } }
    public var menuPrimaryAction: MenuPrimaryAction {
        guard running, !sleeping, !automation.locked, !isBusy, !isStale,
            !connection.changing, connection.confirmed
        else { return .disabled }
        if let active { return stopPrimaryAction(worklogID: active.id) }
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
        return tasks.first { $0.id == active.taskId }?.name ?? "Unknown task"
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
                if $0.name == $1.name { return taskIDComesBefore($0.id, $1.id) }
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
        guard lifecycle == .idle else { return }
        lifecycle = .running
        dailyTotals.updateDay(at: clock.now)
        updateRolloverTimer()
        beginOperation()
        updateDisplayTimer()
        let token = generation
        let settings = connection.settings
        Task { [weak self] in
            guard let self, isCurrent(token) else { return }
            defer { finishOperation(token: token) }
            do {
                let snapshot = try await client.openConfigured(settings)
                guard acceptTaskListIfCurrent(snapshot, token: token) else { return }
            } catch {
                guard isCurrent(token) else { return }
                recordConnectionFailure(error)
            }
        }
    }

    public func shutdown() {
        guard lifecycle != .stopped else { return }
        lifecycle = .stopped
        generation += 1
        cancelPendingControlCommand("The tracker has stopped.")
        history.invalidate()
        automation.cancel()
        creation.reset()
        rename.reset()
        correction.reset()
        move.reset()
        archiving.reset()
        bulkArchiving.reset()
        pendingRefresh = false
        cancelTimers()
        isBusy = false
        connection.changing = false
        publish()
    }

    public func setWindowVisible(_ visible: Bool) {
        guard lifecycle != .stopped else { return }
        let previouslyVisible = uiVisible
        windowVisible = visible
        visibilityChanged(previouslyVisible: previouslyVisible)
    }

    public func menuOpened() {
        guard lifecycle != .stopped else { return }
        let previouslyVisible = uiVisible
        menuDepth += 1
        visibilityChanged(previouslyVisible: previouslyVisible)
    }

    public func menuClosed() {
        guard lifecycle != .stopped else { return }
        let previouslyVisible = uiVisible
        menuDepth = max(0, menuDepth - 1)
        visibilityChanged(previouslyVisible: previouslyVisible)
    }

    public func sleep() {
        guard running else { return }
        sleeping = true
        bulkArchiving.sleep()
        cancelPendingControlCommand("The command was cancelled because the computer went to sleep.")
        cancelTimers()
        publish()
    }

    public func wake() {
        guard running else { return }
        sleeping = false
        tracking.resetElapsedAnchor(clock: clock)
        dailyTotals.updateDay(at: clock.now)
        dailyTotals.reanchor(clock: clock)
        updateDisplayTimer()
        updateRolloverTimer()
        if rename.hasPendingIntent || creation.pending || correction.pending || move.pending || archiving.pending
            || bulkArchiving.pendingSubmit
        {
            if drainAutomation() { return }
            if drainBulkArchiving() { return }
            if drainArchiving() { return }
            if drainRename() { return }
            if drainCreation() { return }
            if drainCorrection() { return }
            if drainMove() { return }
        }
        refresh()
    }

    public var bulkTaskArchiving: BulkTaskArchivingPresentation { bulkArchiving.presentation }
    public var canOpenBulkTaskArchiving: Bool { canBeginTaskArchiving }

    public func openBulkTaskArchiving() {
        guard canOpenBulkTaskArchiving else { return }
        bulkArchiving.open()
        drainBulkArchivePreview()
        publish()
    }
    public func updateBulkArchiveDays(_ text: String) {
        guard running else { return }
        bulkArchiving.updateDays(text)
        drainBulkArchivePreview()
        publish()
    }
    public func refreshBulkArchivePreview() {
        guard running else { return }
        bulkArchiving.requestPreview()
        drainBulkArchivePreview()
        publish()
    }
    public func cancelBulkTaskArchiving() {
        guard running else { return }
        bulkArchiving.cancel()
        publish()
    }
    public func submitBulkTaskArchiving() {
        guard running, !sleeping, connection.confirmed, !connection.changing,
            bulkArchiving.submit()
        else { return }
        drainBulkArchiving()
        publish()
    }

    public var taskArchiving: TaskArchivingPresentation { archiving.presentation }

    private var canBeginTaskArchiving: Bool {
        running && !sleeping && connection.confirmed && !connection.changing && (!isBusy || !operationBlocksControls)
            && pendingControlCommand == nil && !archiving.ownsPresentation && !bulkArchiving.ownsPresentation
            && !creation.presentation.isPresented && !creation.blocksConnectionChange
            && !rename.presentation.isPresented && !rename.blocksConnectionChange && !correction.blocksConnectionChange
            && !move.blocksConnectionChange
    }

    var taskArchivingAvailability: [String: Bool] {
        let available = canBeginTaskArchiving
        let runningTaskID = active?.taskId
        return Dictionary(
            uniqueKeysWithValues: tasks.map { task in
                (task.id, available && (task.archived || task.id != runningTaskID))
            })
    }

    public func canArchiveTask(taskID: String) -> Bool {
        canBeginTaskArchiving && active?.taskId != taskID && hasTask(withID: taskID, archived: false, in: tasks)
    }

    public func canUnarchiveTask(taskID: String) -> Bool {
        canBeginTaskArchiving && hasTask(withID: taskID, archived: true, in: tasks)
    }

    public func openTaskArchive(taskID: String) {
        guard canArchiveTask(taskID: taskID), let task = tasks.first(where: { $0.id == taskID }) else { return }
        archiving.open(task, action: .archive)
        publish()
    }

    public func unarchiveTask(taskID: String) {
        guard canUnarchiveTask(taskID: taskID), let task = tasks.first(where: { $0.id == taskID }) else { return }
        archiving.open(task, action: .unarchive)
        guard archiving.submit(at: commandTimestamp(clock.now), immediate: true) else { return }
        drainArchiving()
        publish()
    }

    public func cancelTaskArchiving() {
        guard running else { return }
        archiving.cancel()
        publish()
    }

    public func submitTaskArchiving() {
        guard running, !sleeping, connection.confirmed, !connection.changing,
            archiving.submit(at: commandTimestamp(clock.now))
        else { return }
        drainArchiving()
        publish()
    }

    public func reopenTaskArchiving() {
        guard running, !sleeping, !connection.changing,
            !creation.presentation.isPresented, !creation.blocksConnectionChange,
            !rename.presentation.isPresented, !rename.blocksConnectionChange,
            !correction.blocksConnectionChange, !move.blocksConnectionChange, !bulkArchiving.ownsPresentation
        else { return }
        archiving.reopen()
        publish()
    }

    public func reviewLatestTaskArchiving() {
        guard running, let taskID = archiving.original?.id else { return }
        archiving.reviewLatest(tasks.first { $0.id == taskID }, active: active)
        publish()
    }

    public func openTaskCreation() {
        guard canOpenTaskCreation else { return }
        creation.open()
        publish()
    }

    public func setTaskCreationName(_ name: String) {
        guard running else { return }
        creation.updateName(name)
        publish()
    }

    public func cancelTaskCreation() {
        guard running else { return }
        creation.cancel()
        publish()
    }

    public func submitTaskCreation() {
        guard canOpenTaskCreation else { return }
        guard creation.submit(at: commandTimestamp(clock.now)) else {
            publish()
            return
        }
        drainCreation()
        publish()
    }

    public func openTaskRename(taskID: String? = nil) {
        guard canOpenTaskRename else { return }
        let target = taskID.flatMap { task(withID: $0, in: tasks) } ?? (taskID == nil ? selectedTask : nil)
        rename.open(target)
        publish()
    }

    public func setTaskRenameName(_ name: String) {
        guard running else { return }
        rename.updateName(name)
        publish()
    }

    public func cancelTaskRename() {
        guard running else { return }
        rename.cancel()
        publish()
    }

    public func submitTaskRename() {
        guard canOpenTaskRename else { return }
        guard rename.submit(at: commandTimestamp(clock.now)) else {
            publish()
            return
        }
        drainRename()
        publish()
    }

    public func openWorklogCorrection(worklogID: String) {
        guard canOpenWorklogCorrection else { return }
        let worklog =
            correction.original ?? worklog(withID: worklogID, in: worklogs) ?? (active?.id == worklogID ? active : nil)
        guard let worklog else { return }
        correction.open(
            worklog, taskName: task(withID: worklog.taskId, in: tasks)?.name ?? "Unknown task",
            historyPageLimit: max(1, (worklogs.count + 49) / 50) + 1)
        publish()
    }

    public func openWorklogMove(worklogID: String) {
        guard canOpenWorklogMove else { return }
        let worklog =
            move.original ?? worklog(withID: worklogID, in: worklogs) ?? (active?.id == worklogID ? active : nil)
        guard let worklog else { return }
        move.open(
            worklog, sourceTaskName: task(withID: worklog.taskId, in: tasks)?.name ?? "Unknown task",
            historyPageLimit: max(2, (worklogs.count + 49) / 50 + 1))
        drainMoveSearch()
        publish()
    }

    public func setWorklogMoveQuery(_ query: String) {
        guard running else { return }
        move.updateQuery(query)
        drainMoveSearch()
        publish()
    }

    public func retryWorklogMoveCandidates() {
        guard running else { return }
        move.retrySearch()
        drainMoveSearch()
        publish()
    }

    public func selectWorklogMoveDestination(taskID: String) {
        guard running else { return }
        move.select(taskID)
        publish()
    }

    public func moveWorklogDestinationSelection(by offset: Int) {
        guard running else { return }
        move.moveSelection(by: offset)
        publish()
    }

    public func cancelWorklogMove() {
        guard running else { return }
        move.cancel()
        publish()
    }

    public func submitWorklogMove() {
        guard canOpenWorklogMove else { return }
        guard move.submit() else { return }
        drainMove()
        publish()
    }

    public func reviewLatestWorklogMove() {
        guard running else { return }
        let taskID = move.presentation.latest?.taskId
        move.reviewLatest(taskName: tasks.first { $0.id == taskID }?.name)
        drainMoveSearch()
        publish()
    }

    public func setWorklogCorrectionStart(_ date: Date) {
        guard running else { return }
        correction.updateStart(date)
        publish()
    }

    public func setWorklogCorrectionEnd(_ date: Date) {
        guard running else { return }
        correction.updateEnd(date)
        publish()
    }

    public func cancelWorklogCorrection() {
        guard running else { return }
        correction.cancel()
        publish()
    }

    public func submitWorklogCorrection() {
        guard canOpenWorklogCorrection else { return }
        guard correction.submit(at: clock.now) else { publish(); return }
        drainCorrection()
        publish()
    }

    public func reviewLatestWorklogCorrection() {
        guard running else { return }
        let taskID = correction.presentation.latest?.taskId
        correction.reviewLatest(taskName: tasks.first { $0.id == taskID }?.name)
        publish()
    }

    public func setPauseOnScreenLock(_ enabled: Bool) {
        guard lifecycle != .stopped,
            automation.setEnabled(
                enabled, at: clock.now, active: active,
                ownStartTaskID: pendingOwnStartTaskID, confirmed: connection.confirmed)
        else { return }
        trackingPreferencesRepository?.save(TrackingPreferences(pauseOnScreenLock: enabled))
        drainAutomation()
        publish()
    }

    public func screenLocked(at date: Date) {
        guard lifecycle != .stopped else { return }
        if automation.enabled {
            cancelPendingControlCommand("The command was cancelled because the screen was locked.")
        }
        automation.lock(
            at: date, active: active, ownStartTaskID: pendingOwnStartTaskID,
            confirmed: connection.confirmed)
        drainAutomation()
        publish()
    }

    public func screenUnlocked(at date: Date) {
        guard lifecycle != .stopped else { return }
        automation.unlock(at: date)
        drainAutomation()
        publish()
    }

    public func changeTab(_ newTab: TaskTab) {
        guard lifecycle != .stopped, catalog.changeTab(newTab) else { return }
        requestHistory()
        publish()
    }

    public func select(_ taskID: String?) {
        guard lifecycle != .stopped, catalog.select(taskID) else { return }
        requestHistory()
        publish()
    }

    public func refresh() {
        guard running, !sleeping else { return }
        connection.protocolBlocked = false
        if isBusy { pendingRefresh = true; return }
        beginRefresh()
    }

    public func retryHistory() {
        guard running else { return }
        requestHistory()
        publish()
    }

    public func testConnection(_ settings: ConnectionSettings) async throws {
        guard !bulkArchiving.ownsPresentation else {
            throw BridgeFailure(message: "Close the archive dialog before testing a connection.")
        }
        let operation = try await acquireControlOperation()
        defer { finishOperation(token: operation.token) }
        try Task.checkCancellation()
        guard controlOperationIsCurrent(operation), !sleeping, !bulkArchiving.ownsPresentation else {
            throw CancellationError()
        }
        try await client.test(connection.normalized(settings))
    }

    public func connect(_ settings: ConnectionSettings) async -> Bool {
        guard !bulkArchiving.ownsPresentation else {
            return rejectConnection("Close the archive dialog before changing connections.")
        }
        guard !archiving.blocksConnectionChange else {
            return rejectConnection("Finish or retry task archiving before changing connections.")
        }
        guard !move.blocksConnectionChange else {
            return rejectConnection("Finish or retry worklog moving before changing connections.")
        }
        guard !correction.blocksConnectionChange && !move.blocksConnectionChange else {
            return rejectConnection("Finish worklog editing before changing connections.")
        }
        guard !rename.blocksConnectionChange else {
            return rejectConnection("Finish or retry task renaming before changing connections.")
        }
        guard !creation.blocksConnectionChange else {
            return rejectConnection("Finish or retry task creation before changing connections.")
        }
        automation.cancel()
        let operation: ControlOperation
        do { operation = try await acquireControlOperation() } catch {
            if running { connection.message = "Connection unchanged. \(error.localizedDescription)"; publish() }
            return false
        }
        let token = operation.token
        defer { finishOperation(token: token) }
        guard controlOperationIsCurrent(operation), !sleeping, !Task.isCancelled else { return false }
        guard !rename.blocksConnectionChange, !creation.blocksConnectionChange,
            !correction.blocksConnectionChange && !move.blocksConnectionChange && !archiving.blocksConnectionChange
                && !bulkArchiving.ownsPresentation
        else {
            connection.message = "Finish or retry task editing before changing connections."
            return false
        }
        automation.cancel()
        connection.changing = true
        defer {
            if isCurrent(token) {
                connection.changing = false
            }
        }
        publish()
        guard controlOperationIsCurrent(operation), !sleeping, !Task.isCancelled else { return false }
        do {
            let normalized = try connection.normalized(settings)
            let snapshot = try await client.connect(normalized)
            guard isCurrent(token) else { return false }
            return finishConnection(snapshot, settings: normalized)
        } catch {
            guard isCurrent(token) else { return false }
            connection.message = "Connection unchanged. \(error.localizedDescription)"
            return false
        }
    }

    public func startTracking(taskID: String) {
        guard canStartTracking(taskID: taskID) else { return }
        automation.cancel()
        pendingControlCommand = .tracking(
            .start(
                taskID: taskID, expectedActiveID: active?.id,
                occurredAt: commandTimestamp(clock.now)), queued: isBusy)
        drainControlCommand()
        publish()
    }

    public func stopTracking(worklogID: String) {
        guard canStopTracking, active?.id == worklogID else { return }
        automation.cancel()
        pendingControlCommand = .tracking(
            .stop(worklogID: worklogID, occurredAt: commandTimestamp(clock.now)), queued: isBusy)
        drainControlCommand()
        publish()
    }

    public func dismissTrackingError() {
        guard lifecycle != .stopped else { return }
        tracking.error = nil
        publish()
    }

    public func loadOlder() {
        guard running, !sleeping, !isBlockingControls, let taskID = selectedTaskID, let cursor = nextCursor else {
            return
        }
        pendingControlCommand = .history(taskID: taskID, cursor: cursor, historyToken: history.generation)
        drainControlCommand()
        publish()
    }

    private func isCurrent(_ token: Int) -> Bool { running && token == generation }

    private func controlOperationIsCurrent(_ operation: ControlOperation) -> Bool {
        isCurrent(operation.token) && operation.controlGeneration == controlGeneration
    }

    private func acquireControlOperation() async throws -> ControlOperation {
        guard running, !sleeping, !isBlockingControls else {
            throw BridgeFailure(message: "Wait for the current request to finish.")
        }
        if !isBusy {
            try Task.checkCancellation()
            let operation = ControlOperation(token: generation, controlGeneration: controlGeneration)
            beginOperation()
            return operation
        }
        let id = UUID()
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                guard !Task.isCancelled else { continuation.resume(throwing: CancellationError()); return }
                pendingControlCommand = .access(id: id, continuation: continuation)
                publish()
            }
        } onCancel: {
            Task { @MainActor [weak self] in
                guard let self, case .access(let pendingID, let continuation) = pendingControlCommand,
                    pendingID == id
                else { return }
                pendingControlCommand = nil
                continuation.resume(throwing: CancellationError())
                publish()
            }
        }
    }

    private func cancelPendingControlCommand(_ message: String) {
        controlGeneration += 1
        guard let command = pendingControlCommand else { return }
        pendingControlCommand = nil
        switch command {
        case .tracking: tracking.error = message
        case .access(_, let continuation): continuation.resume(throwing: CancellationError())
        case .history: break
        }
    }

    @discardableResult
    private func drainControlCommand() -> Bool {
        guard running, !sleeping, !isBusy, let command = pendingControlCommand else { return false }
        pendingControlCommand = nil
        switch command {
        case .access(_, let continuation):
            let operation = ControlOperation(token: generation, controlGeneration: controlGeneration)
            beginOperation()
            continuation.resume(returning: operation)
        case .tracking(let intent, let queued):
            // A history page cannot confirm the timer targeted by a queued click.
            executeTracking(intent, requiresResources: queued && !operationHasResources)
        case .history(let taskID, let cursor, let historyToken):
            guard taskID == selectedTaskID, historyToken == history.generation, cursor == nextCursor else {
                return false
            }
            loadHistory(taskID: taskID, cursor: cursor, historyToken: historyToken)
        }
        return true
    }

    private func beginOperation(blocksControls: Bool = true) {
        isBusy = true
        operationBlocksControls = blocksControls
        operationHasResources = false
        cancelPolling()
        publish()
    }

    private func finishOperation(token: Int) {
        guard isCurrent(token) else { return }
        isBusy = false
        if drainAutomation() { return }
        if drainBulkArchiving() { return }
        if drainArchiving() { return }
        if drainRename() { return }
        if drainCreation() { return }
        if drainCorrection() { return }
        if drainMove() { return }
        if drainControlCommand() { return }
        if !sleeping {
            if drainBulkArchivePreview() { return }
            if drainMoveSearch() { return }
            if pendingRefresh {
                pendingRefresh = false
                beginRefresh()
            } else if reports != nil && dailyTotals.pending && connection.confirmed && !connection.stale {
                beginReport()
            } else if history.pending {
                history.pending = false
                if let taskID = selectedTaskID {
                    loadHistory(taskID: taskID, cursor: nil, historyToken: history.generation, blocksControls: false)
                } else {
                    schedulePolling()
                }
            } else {
                schedulePolling()
            }
        }
        publish()
    }

    @discardableResult
    private func drainBulkArchivePreview() -> Bool {
        guard running, !sleeping, !isBusy,
            let search = bulkArchiving.takeSearch(asOf: commandTimestamp(clock.now))
        else { return false }
        let token = generation
        beginOperation(blocksControls: false)
        Task { [weak self] in
            guard let self, isCurrent(token) else { return }
            defer { finishOperation(token: token) }
            do {
                if connection.stale {
                    let snapshot = try await client.refreshTaskList()
                    guard isCurrent(token), !sleeping else { return }
                    acceptTaskList(snapshot)
                }
                guard bulkArchiving.matches(search) else { return }
                let currentSearch = BulkTaskArchivingState.Search(
                    generation: search.generation, days: search.days,
                    asOf: commandTimestamp(clock.now))
                let value = try await client.previewInactiveTasks(
                    inactiveDays: currentSearch.days, asOf: currentSearch.asOf)
                guard isCurrent(token), !sleeping else { return }
                try bulkArchiving.accept(value, search: currentSearch)
            } catch {
                guard isCurrent(token), !sleeping else { return }
                bulkArchiving.failSearch(error, search: search)
                if bulkArchiving.matches(search), let failure = error as? BridgeFailure,
                    failure.requiresRefresh || failure.uncertain || failure.kind == "unavailable"
                        || failure.kind == "protocol"
                {
                    recordConnectionFailure(error)
                }
            }
        }
        return true
    }

    @discardableResult
    private func drainBulkArchiving() -> Bool {
        guard running, !sleeping, !isBusy, let submission = bulkArchiving.takeSubmission() else { return false }
        let token = generation
        beginOperation()
        Task { [weak self] in
            guard let self, isCurrent(token) else { return }
            defer { finishOperation(token: token) }
            guard !sleeping, bulkArchiving.beginSubmission(submission) else { return }
            do {
                let result = try await client.archiveInactiveTasks(preview: submission.preview)
                guard isCurrent(token) else { return }
                guard result.archivedCount >= 0 else {
                    throw BridgeFailure(
                        message: "The tracker returned an invalid archived count.", kind: "protocol",
                        requiresRefresh: true)
                }
                bulkArchiving.complete(count: result.archivedCount)
                await refreshAfterCommit(token: token)
            } catch {
                guard isCurrent(token) else { return }
                await reconcileWriteFailure(token: token)
                guard isCurrent(token) else { return }
                bulkArchiving.failSubmission(error)
            }
        }
        return true
    }

    @discardableResult
    private func drainCreation() -> Bool {
        guard running, !sleeping, !isBusy, creation.pending, let intent = creation.intent else { return false }
        creation.pending = false
        let token = generation
        beginOperation()
        Task { [weak self] in
            guard let self, isCurrent(token) else { return }
            defer { finishOperation(token: token) }
            var commandStarted = false
            do {
                if !connection.confirmed || connection.stale {
                    let snapshot: TaskListResources
                    do {
                        snapshot = try await client.refreshTaskList()
                    } catch {
                        guard isCurrent(token) else { return }
                        recordConnectionFailure(error)
                        throw error
                    }
                    guard isCurrent(token) else { return }
                    acceptTaskList(snapshot)
                }
                if sleeping {
                    creation.pending = true
                    return
                }
                commandStarted = true
                let result = try await client.createTask(name: intent.name, occurredAt: intent.occurredAt)
                guard isCurrent(token) else { return }
                let created = result.task
                guard !created.id.isEmpty else {
                    throw BridgeFailure(
                        message: "The creation response does not contain the created task.",
                        kind: "protocol", uncertain: true, requiresRefresh: true)
                }
                creation.reset()
                await refreshAfterCommit(token: token)
                guard isCurrent(token) else { return }
                let confirmed = task(withID: created.id, in: tasks) ?? created
                let changedTab = catalog.changeTab(confirmed.archived ? .archived : .active)
                let changedSelection = catalog.select(created.id)
                if changedTab || changedSelection { requestHistory() }
            } catch {
                guard isCurrent(token) else { return }
                let unresolved = TaskCreationState.requiresRecovery(error)
                if unresolved && commandStarted { await reconcileWriteFailure(token: token) }
                guard isCurrent(token) else { return }
                creation.fail(error, retainIntent: unresolved)
                publish()
            }
        }
        return true
    }

    @discardableResult
    private func reconcileWriteFailure(token: Int) async -> TaskListResources? {
        connection.stale = true
        publish()
        do {
            // A lost write response can follow a committed change.
            let snapshot = try await client.refreshTaskList()
            guard isCurrent(token) else { return nil }
            acceptTaskList(snapshot)
            return snapshot
        } catch {
            guard isCurrent(token) else { return nil }
            recordConnectionFailure(error)
            return nil
        }
    }

    private func refreshAfterCommit(token: Int) async {
        do {
            let resources = try await client.refreshTaskList()
            guard isCurrent(token) else { return }
            acceptTaskList(resources)
        } catch {
            guard isCurrent(token) else { return }
            recordConnectionFailure(error)
            publish()
        }
    }

    private func finishTaskArchiving(_ intent: TaskArchivingState.Intent) {
        if intent.action == .unarchive { catalog.rememberRestoredTask(intent.taskID) }
        archiving.reset()
    }

    @discardableResult
    private func drainArchiving() -> Bool {
        guard running, !sleeping, !isBusy, let intent = archiving.takePendingIntent() else { return false }
        let token = generation
        beginOperation()
        Task { [weak self] in
            guard let self, isCurrent(token) else { return }
            defer { finishOperation(token: token) }
            var commandStarted = false
            do {
                let snapshot: TaskListResources
                do { snapshot = try await client.refreshTaskList() } catch {
                    guard isCurrent(token) else { return }
                    recordConnectionFailure(error)
                    throw error
                }
                guard isCurrent(token) else { return }
                acceptTaskList(snapshot)
                switch archiving.preflight(intent, snapshot: snapshot) {
                case .applied: finishTaskArchiving(intent); return
                case .review: archiving.setStatusVisibility(windowVisible); return
                case .apply: break
                }
                if sleeping { archiving.deferUntilWake(); return }
                commandStarted = true
                let result: TaskCommandResult
                switch intent.action {
                case .archive:
                    result = try await client.archiveTask(taskID: intent.taskID, occurredAt: intent.occurredAt)
                case .unarchive:
                    result = try await client.unarchiveTask(taskID: intent.taskID, occurredAt: intent.occurredAt)
                }
                guard isCurrent(token) else { return }
                guard result.task.id == intent.taskID,
                    result.task.archived == intent.action.desiredArchived
                else {
                    throw BridgeFailure(
                        message: "The response does not confirm the task's archive state.",
                        kind: "protocol", uncertain: true, requiresRefresh: true)
                }
                archiving.reset()
                await refreshAfterCommit(token: token)
                guard isCurrent(token) else { return }
                if intent.action == .unarchive { catalog.rememberRestoredTask(intent.taskID) }
            } catch {
                guard isCurrent(token) else { return }
                let confirmed = commandStarted ? await reconcileWriteFailure(token: token) : nil
                guard isCurrent(token) else { return }
                if let confirmed {
                    switch archiving.preflight(intent, snapshot: confirmed) {
                    case .applied: finishTaskArchiving(intent); return
                    case .review: archiving.setStatusVisibility(windowVisible); return
                    case .apply: break
                    }
                }
                archiving.fail(
                    error,
                    retainIntent: TaskArchivingState.requiresRecovery(error) || (commandStarted && confirmed == nil))
                archiving.setStatusVisibility(windowVisible)
                publish()
            }
        }
        return true
    }

    @discardableResult
    private func drainRename() -> Bool {
        guard running, !sleeping, !isBusy, let intent = rename.takePendingIntent() else { return false }
        let token = generation
        beginOperation()
        Task { [weak self] in
            guard let self, isCurrent(token) else { return }
            defer { finishOperation(token: token) }
            var commandStarted = false
            do {
                let snapshot: TaskListResources
                do {
                    snapshot = try await client.refreshTaskList()
                } catch {
                    guard isCurrent(token) else { return }
                    recordConnectionFailure(error)
                    throw error
                }
                guard isCurrent(token) else { return }
                acceptTaskList(snapshot)
                guard renameCanApply(intent, snapshot: snapshot) else { return }
                if sleeping {
                    rename.deferUntilWake()
                    return
                }
                commandStarted = true
                let result = try await client.renameTask(
                    taskID: intent.taskID, name: intent.name,
                    occurredAt: intent.occurredAt)
                guard isCurrent(token) else { return }
                guard result.task.id == intent.taskID, result.task.name == intent.desiredName else {
                    throw BridgeFailure(
                        message: "The rename response does not contain the updated task.",
                        kind: "protocol", uncertain: true, requiresRefresh: true)
                }
                rename.reset()
                await refreshAfterCommit(token: token)
            } catch {
                guard isCurrent(token) else { return }
                let uncertain = TaskNameEditingPolicy.requiresRecovery(error)
                let confirmed = commandStarted ? await reconcileWriteFailure(token: token) : nil
                guard isCurrent(token) else { return }
                if let confirmed, !renameCanApply(intent, snapshot: confirmed) { return }
                if let failure = error as? BridgeFailure, failure.kind == "conflict" {
                    rename.requireReview(
                        "The task changed on another client. Cancel and reopen the editor to review its current state.")
                } else {
                    rename.fail(
                        error,
                        retainIntent: retainRenameIntent(
                            uncertain: uncertain, commandStarted: commandStarted, confirmed: confirmed))
                }
                publish()
            }
        }
        return true
    }

    private func renameCanApply(_ intent: TaskRenameState.Intent, snapshot: TaskListResources) -> Bool {
        guard let task = task(withID: intent.taskID, in: snapshot.catalog.value) else {
            return requireRenameReview("The task no longer exists. Cancel the editor and refresh the task list.")
        }
        if task.name == intent.desiredName {
            rename.reset()
            return false
        }
        guard task.name == intent.originalName else {
            return requireRenameReview(
                "The task name changed on another client. Cancel and reopen the editor to review its current name.")
        }
        return true
    }

    @discardableResult
    private func drainMoveSearch() -> Bool {
        guard running, !sleeping, !isBusy, let initialSearch = move.takeSearch() else { return false }
        let token = generation
        beginOperation(blocksControls: false)
        Task { [weak self] in
            guard let self, isCurrent(token) else { return }
            defer { finishOperation(token: token) }
            var search = initialSearch
            do {
                if search.refresh {
                    let snapshot = try await client.refreshTaskList()
                    guard isCurrent(token) else { return }
                    acceptTaskList(snapshot)
                    move.didRefresh(search)
                }
                guard prepareMoveSearch() else { return }
                guard let currentSearch = move.currentSearch(replacing: search) else { return }
                search = currentSearch
                let values = try await client.moveCandidates(sourceTaskID: search.sourceTaskID, query: search.query)
                guard isCurrent(token) else { return }
                if sleeping { move.deferSearch(); return }
                move.accept(values, search: search)
            } catch {
                guard isCurrent(token) else { return }
                move.failSearch(error, search: search)
            }
        }
        return true
    }

    private func currentMoveWorklog(
        _ intent: WorklogMoveState.Intent,
        snapshot: TaskListResources, token: Int, includeDestination: Bool
    ) async throws -> WorklogItem? {
        if let active = snapshot.tracking.value, active.id == intent.expected.id { return active }
        let taskIDs =
            includeDestination ? [intent.destinationTaskID, intent.expected.taskId] : [intent.expected.taskId]
        var verificationFailure: BridgeFailure?
        for taskID in taskIDs {
            guard containsTask(withID: taskID, in: snapshot.catalog.value) else { continue }
            var cursor: String?
            var seen = Set<String>()
            var pagesRead = 0
            let loadedPages = loadedHistoryPages(for: taskID)
            let limit = max(intent.historyPageLimit, loadedPages + 1)
            repeat {
                guard withinHistoryLimit(pagesRead, limit: limit) else {
                    verificationFailure = BridgeFailure(
                        message:
                            "Could not verify this worklog within the history limit. Cancel and reopen the mover before retrying."
                    )
                    break
                }
                pagesRead += 1
                let page = try await client.history(taskID: taskID, cursor: cursor)
                guard isCurrent(token), !sleeping else { return nil }
                if let worklog = worklog(withID: intent.expected.id, in: page.worklogs) { return worklog }
                cursor = page.nextCursor
                if let cursor, !seen.insert(cursor).inserted {
                    verificationFailure = BridgeFailure(
                        message: "Worklog history returned a repeated cursor.", kind: "protocol")
                    break
                }
            } while hasMoreHistory(cursor)
        }
        if let verificationFailure { throw verificationFailure }
        return nil
    }

    private func finishMove(_ intent: WorklogMoveState.Intent, snapshot: TaskListResources? = nil) {
        move.reset()
        if let snapshot { acceptTaskList(snapshot) }
        if selectedTaskID == intent.expected.taskId || selectedTaskID == intent.destinationTaskID {
            history.clearAfterCorrection()
        }
        requestHistory()
    }

    private func moveCanApply(
        _ intent: WorklogMoveState.Intent, latest: WorklogItem?,
        snapshot: TaskListResources, resolveCommitted: Bool
    ) -> Bool {
        if resolveCommitted, let latest, intent.matchesReplacement(latest) {
            finishMove(intent)
            return false
        }
        guard let latest else {
            return requireMoveReview(
                latest: nil, message: "This worklog was moved or deleted. Cancel the mover and refresh history.")
        }
        guard latest.id == intent.expected.id, latest.taskId == intent.expected.taskId,
            sameWorklogTimestamp(latest.start, intent.expected.start),
            sameWorklogTimestamp(latest.end, intent.expected.end)
        else {
            return requireMoveReview(latest: latest, message: "This worklog changed. Review it before moving.")
        }
        guard hasTask(withID: intent.destinationTaskID, archived: false, in: snapshot.catalog.value) else {
            return failMoveDestination()
        }
        return true
    }

    @discardableResult
    private func drainMove() -> Bool {
        guard running, !sleeping, !isBusy, let intent = move.takePendingIntent() else { return false }
        let token = generation
        beginOperation()
        Task { [weak self] in
            guard let self, isCurrent(token) else { return }
            defer { finishOperation(token: token) }
            var commandStarted = false
            do {
                let snapshot = try await client.refreshTaskList()
                guard isCurrent(token) else { return }
                acceptTaskList(snapshot)
                if sleeping { move.deferUntilWake(); return }
                let latest = try await currentMoveWorklog(
                    intent, snapshot: snapshot, token: token, includeDestination: move.mayHaveCommitted)
                guard isCurrent(token) else { return }
                if sleeping { move.deferUntilWake(); return }
                guard moveCanApply(intent, latest: latest, snapshot: snapshot, resolveCommitted: move.mayHaveCommitted)
                else { return }
                commandStarted = true
                move.commandStarted()
                let result = try await client.moveWorklog(
                    expected: intent.expected, destinationTaskID: intent.destinationTaskID)
                guard isCurrent(token) else { return }
                guard intent.matchesReplacement(result.worklog) else {
                    throw BridgeFailure(
                        message: "The move response does not contain the moved worklog.",
                        kind: "protocol", uncertain: true, requiresRefresh: true)
                }
                finishMove(intent)
                await refreshAfterCommit(token: token)
            } catch {
                guard isCurrent(token) else { return }
                let failure = error as? BridgeFailure
                let uncertain =
                    failure == nil || failure?.uncertain == true || failure?.kind == "unavailable"
                    || failure?.kind == "protocol"
                var reconciled = false
                if commandStarted {
                    if let snapshot = await reconcileWriteFailure(token: token) {
                        if sleeping { move.fail(error, retainIntent: true); return }
                        do {
                            let latest = try await currentMoveWorklog(
                                intent, snapshot: snapshot, token: token, includeDestination: true)
                            guard isCurrent(token) else { return }
                            if sleeping { move.fail(error, retainIntent: true); return }
                            reconciled = true
                            guard moveCanApply(intent, latest: latest, snapshot: snapshot, resolveCommitted: uncertain)
                            else { return }
                            if isWorklogChangedFailure(failure) {
                                move.requireReview(
                                    latest: latest, message: "This worklog changed. Review it before moving.")
                                return
                            }
                        } catch {
                            guard isCurrent(token) else { return }
                            recordConnectionFailure(error)
                        }
                    }
                } else if uncertain || failure?.requiresRefresh == true {
                    recordConnectionFailure(error)
                }
                guard isCurrent(token) else { return }
                move.fail(error, retainIntent: uncertain || (commandStarted && !reconciled))
            }
        }
        return true
    }

    private func currentCorrectionWorklog(
        _ intent: WorklogCorrectionState.Intent,
        snapshot: TaskListResources, token: Int
    ) async throws -> WorklogItem? {
        if let active = snapshot.tracking.value, active.id == intent.expected.id { return active }
        let taskID = intent.expected.taskId
        if containsTask(withID: taskID, in: snapshot.catalog.value) {
            var cursor: String?
            var seen = Set<String>()
            let loadedPages = loadedHistoryPages(for: taskID)
            let pageLimit = max(intent.historyPageLimit, loadedPages + 1)
            var pagesRead = 0
            repeat {
                guard withinHistoryLimit(pagesRead, limit: pageLimit) else {
                    throw BridgeFailure(
                        message:
                            "Could not verify this worklog within the history limit. Cancel and reopen the editor before retrying."
                    )
                }
                pagesRead += 1
                let page = try await client.history(taskID: taskID, cursor: cursor)
                guard isCurrent(token), !sleeping else { return nil }
                if let worklog = worklog(withID: intent.expected.id, in: page.worklogs) { return worklog }
                cursor = page.nextCursor
                if let cursor, !seen.insert(cursor).inserted {
                    throw BridgeFailure(message: "Worklog history returned a repeated cursor.", kind: "protocol")
                }
            } while hasMoreHistory(cursor)
        }
        return nil
    }

    private func correctionCanApply(
        _ intent: WorklogCorrectionState.Intent, latest: WorklogItem?,
        resolveCommitted: Bool
    ) -> Bool {
        guard let latest else {
            return requireCorrectionReview(
                latest: nil, message: "This worklog was moved or deleted. Cancel the editor and refresh history.")
        }
        if resolveCommitted && intent.matchesReplacement(latest) {
            correction.reset()
            if selectedTaskMatches(intent.expected.taskId) { history.clearAfterCorrection() }
            requestHistory()
            return false
        }
        guard latest == intent.expected else {
            return requireCorrectionReview(
                latest: latest, message: "This worklog changed. Review its latest times before saving.")
        }
        return true
    }

    @discardableResult
    private func drainCorrection() -> Bool {
        guard running, !sleeping, !isBusy, let intent = correction.takePendingIntent() else { return false }
        let token = generation
        beginOperation()
        Task { [weak self] in
            guard let self, isCurrent(token) else { return }
            defer { finishOperation(token: token) }
            var commandStarted = false
            do {
                let snapshot = try await client.refreshTaskList()
                guard isCurrent(token) else { return }
                acceptTaskList(snapshot)
                if sleeping { correction.deferUntilWake(); return }
                let latest = try await currentCorrectionWorklog(intent, snapshot: snapshot, token: token)
                guard isCurrent(token) else { return }
                if sleeping { correction.deferUntilWake(); return }
                guard correctionCanApply(intent, latest: latest, resolveCommitted: true) else { return }
                commandStarted = true
                let result = try await client.correctWorklog(
                    expected: intent.expected,
                    replacementStart: intent.replacementStart,
                    replacementEnd: intent.replacementEnd,
                    occurredAt: intent.occurredAt)
                guard isCurrent(token) else { return }
                guard intent.matchesReplacement(result.worklog) else {
                    throw BridgeFailure(
                        message: "The correction response does not contain the updated worklog.",
                        kind: "protocol", uncertain: true, requiresRefresh: true)
                }
                correction.reset()
                await refreshAfterCommit(token: token)
                guard isCurrent(token) else { return }
                if selectedTaskMatches(intent.expected.taskId) { history.clearAfterCorrection() }
                requestHistory()
            } catch {
                guard isCurrent(token) else { return }
                let failure = error as? BridgeFailure
                let uncertain =
                    failure == nil || failure?.uncertain == true || failure?.kind == "unavailable"
                    || failure?.kind == "protocol"
                var reconciled = false
                if commandStarted {
                    if let snapshot = await reconcileWriteFailure(token: token) {
                        if sleeping {
                            correction.fail(error, retainIntent: true)
                            publish()
                            return
                        }
                        do {
                            let latest = try await currentCorrectionWorklog(intent, snapshot: snapshot, token: token)
                            guard isCurrent(token) else { return }
                            if sleeping {
                                correction.fail(error, retainIntent: true)
                                publish()
                                return
                            }
                            reconciled = true
                            guard correctionCanApply(intent, latest: latest, resolveCommitted: uncertain) else {
                                return
                            }
                            if isWorklogChangedFailure(failure) {
                                correction.requireReview(
                                    latest: latest,
                                    message: "This worklog changed. Review its latest times before saving.")
                                publish()
                                return
                            }
                        } catch {
                            guard isCurrent(token) else { return }
                            recordConnectionFailure(error)
                        }
                    }
                } else if uncertain || failure?.requiresRefresh == true {
                    recordConnectionFailure(error)
                }
                guard isCurrent(token) else { return }
                correction.fail(error, retainIntent: uncertain || (commandStarted && !reconciled))
                publish()
            }
        }
        return true
    }

    @discardableResult
    private func drainAutomation() -> Bool {
        guard running, !isBusy, connection.confirmed, automation.enabled else { return false }
        let action: TrackingAutomationState.Pause?
        let resumeAt: Date?
        let taskID: String?
        if let pause = automation.pendingPause {
            automation.pendingPause = nil
            action = pause
            resumeAt = nil
            taskID = nil
        } else if !sleeping, !automation.locked, let date = automation.resumeAt,
            let pausedTaskID = automation.pausedTaskID
        {
            automation.resumeAt = nil
            action = nil
            resumeAt = date
            taskID = pausedTaskID
        } else {
            return false
        }
        let automationToken = automation.generation
        let token = generation
        tracking.error = nil
        beginOperation()
        Task { [weak self] in
            guard let self, isCurrent(token) else { return }
            defer { finishOperation(token: token) }
            do {
                // Reconcile before an automatic write. A second client may have
                // changed tracking while this request waited behind another one.
                let snapshot = try await client.refreshTaskList()
                guard isCurrent(token) else { return }
                acceptTaskList(snapshot)
                guard automationToken == automation.generation, automation.enabled else { return }
                if let action {
                    guard let worklog = snapshot.tracking.value,
                        pauseTargetsWorklog(action, worklog: worklog)
                    else { return }
                    guard let start = timestamp(worklog.start) else {
                        tracking.error = "Cannot pause tracking because its start time is invalid."
                        return
                    }
                    guard pauseTimeIsValid(start, occurredAt: action.occurredAt) else {
                        tracking.error = "Cannot pause tracking before the worklog start time."
                        return
                    }
                    let result = try await client.pauseTracking(
                        worklogID: worklog.id,
                        occurredAt: commandTimestamp(action.occurredAt))
                    guard isCurrent(token) else { return }
                    await refreshAfterCommit(token: token)
                    guard isCurrent(token), automationIsCurrent(automationToken) else { return }
                    if canRetainPausedTask(result) {
                        automation.pausedTaskID = worklog.taskId
                    } else {
                        automation.cancel()
                    }
                } else if let taskID, let resumeAt {
                    guard prepareAutomationResume(snapshot, taskID: taskID) else { return }
                    guard !automation.locked, !sleeping else { return }
                    automation.pausedTaskID = nil
                    pendingOwnStartTaskID = taskID
                    defer { pendingOwnStartTaskID = nil }
                    let resumed = try await client.resumeTracking(
                        taskID: taskID,
                        occurredAt: commandTimestamp(resumeAt))
                    guard isCurrent(token) else { return }
                    automation.acknowledgeOwnStart(resumed.active, taskID: taskID)
                    await refreshAfterCommit(token: token)
                }
            } catch {
                guard isCurrent(token) else { return }
                if automationToken == automation.generation { automation.cancel() }
                let message = error.localizedDescription
                await reconcileWriteFailure(token: token)
                guard isCurrent(token) else { return }
                tracking.error = message
            }
            publish()
        }
        return true
    }

    private func beginRefresh() {
        dailyTotals.updateDay(at: clock.now)
        if reports != nil, connection.confirmed, !connection.stale {
            beginReport()
            return
        }
        beginOperation(blocksControls: false)
        let token = generation
        let settings = connection.settings
        Task { [weak self] in
            guard let self, isCurrent(token) else { return }
            defer { finishOperation(token: token) }
            do {
                let snapshot = try await client.refreshTaskList(settings: settings)
                guard acceptTaskListIfCurrent(snapshot, token: token) else { return }
            } catch {
                guard isCurrent(token) else { return }
                recordConnectionFailure(error)
            }
        }
    }

    private func executeTracking(_ command: TrackingCommand, requiresResources: Bool) {
        tracking.error = nil
        let token = generation
        let commandGeneration = controlGeneration
        beginOperation()
        Task { [weak self] in
            guard let self, isCurrent(token) else { return }
            defer {
                pendingOwnStartTaskID = nil
                finishOperation(token: token)
            }
            var writeStarted = false
            do {
                guard trackingControlIsCurrent(commandGeneration) else {
                    throw BridgeFailure(
                        message: "Tracking changed or became unavailable while the command was waiting. Try again.")
                }
                if requiresResources {
                    do {
                        let snapshot = try await client.refreshTaskList()
                        guard acceptTaskListIfCurrent(snapshot, token: token) else { return }
                    } catch {
                        guard isCurrent(token) else { return }
                        recordConnectionFailure(error)
                        throw error
                    }
                }
                guard isCurrent(token) else { return }
                guard trackingControlIsCurrent(commandGeneration) else {
                    throw BridgeFailure(
                        message: "Tracking changed or became unavailable while the command was waiting. Try again.")
                }
                let result: TrackingCommandResult
                switch command {
                case .start(let taskID, let expectedActiveID, let occurredAt):
                    guard
                        startCommandCanApply(taskID: taskID, expectedActiveID: expectedActiveID, occurredAt: occurredAt)
                    else {
                        throw BridgeFailure(
                            message: "Tracking changed while Start was waiting. Review the current timer and try again."
                        )
                    }
                    pendingOwnStartTaskID = taskID
                    writeStarted = true
                    result = try await client.startTracking(
                        taskID: taskID, expectedActiveID: expectedActiveID,
                        occurredAt: occurredAt)
                    guard isCurrent(token) else { return }
                    automation.acknowledgeOwnStart(result.active, taskID: taskID)
                case .stop(let worklogID, let occurredAt):
                    guard stopCommandCanApply(worklogID: worklogID, occurredAt: occurredAt) else {
                        throw BridgeFailure(
                            message: "Tracking changed while Stop was waiting. Review the current timer and try again.")
                    }
                    writeStarted = true
                    result = try await client.stopTracking(worklogID: worklogID, occurredAt: occurredAt)
                }
                guard isCurrent(token) else { return }
                await refreshAfterCommit(token: token)
            } catch {
                guard isCurrent(token) else { return }
                let message = error.localizedDescription
                if writeStarted { await reconcileWriteFailure(token: token) }
                guard isCurrent(token) else { return }
                tracking.error = message
            }
        }
    }

    private func trackingTimestampIsValid(_ occurredAt: String) -> Bool {
        guard let date = timestamp(occurredAt), let start = timestamp(active?.start) else { return false }
        return date >= start
    }

    private func acceptTaskList(_ snapshot: TaskListResources, requestReport: Bool = true) {
        operationHasResources = true
        taskCatalogResource = snapshot.catalog
        trackingResource = snapshot.tracking
        dailyTotals.updateDay(at: clock.now)
        let retryFailedHistory = connection.stale && history.unavailable
        connection.acceptTaskList()
        lastTracked.observe(snapshot, settings: connection.settings)
        let previousActive = active
        automation.observeActive(snapshot.tracking.value)
        tracking.apply(snapshot.tracking.value, clock: clock)
        if let active = snapshot.tracking.value { correction.observe(active); move.observe(active) }
        if reports != nil && requestReport { dailyTotals.invalidate() }
        if catalog.apply(snapshot.catalog.value, previousActive: previousActive, active: active) {
            requestHistory()
        } else if !history.unavailable {
            history.error = nil
        }
        if retryFailedHistory && !history.pending { requestHistory() }
        updateDisplayTimer()
        updateRolloverTimer()
        publish()
    }

    private func beginReport() {
        guard let reports, running, !sleeping, let requested = dailyTotals.begin(clock: clock) else {
            return
        }
        let token = generation
        let settings = connection.settings
        beginOperation(blocksControls: false)
        Task { [weak self] in
            guard let self, isCurrent(token) else { return }
            defer { finishOperation(token: token) }
            do {
                let refreshed = try await reports.refreshTaskListWithTotals(
                    settings: settings, start: requested.start,
                    end: requested.end, now: requested.now)
                guard isCurrent(token) else { return }
                try dailyTotals.validate(refreshed.report, tracking: refreshed.taskList.tracking)
                reportResource = refreshed.report
                dailyTotals.accept(
                    refreshed.report, tracking: refreshed.taskList.tracking,
                    requested: requested, clock: clock)
                acceptTaskList(refreshed.taskList, requestReport: false)
            } catch {
                guard isCurrent(token) else { return }
                dailyTotals.fail(error, clock: clock)
                if let failure = error as? BridgeFailure,
                    failure.requiresRefresh || failure.uncertain || failure.kind == "unavailable"
                        || failure.kind == "protocol"
                {
                    recordConnectionFailure(error)
                }
                publish()
            }
        }
    }

    private func recordConnectionFailure(_ failure: Error) {
        connection.recordFailure(failure)
        history.error = failure.localizedDescription
        publish()
    }

    private func requestHistory() {
        history.request(selectedTaskID: selectedTaskID)
        guard running, !isBusy, !sleeping else { return }
        history.pending = false
        if let taskID = selectedTaskID {
            loadHistory(taskID: taskID, cursor: nil, historyToken: history.generation, blocksControls: false)
        }
    }

    private func loadHistory(taskID: String, cursor: String?, historyToken: Int, blocksControls: Bool = true) {
        beginOperation(blocksControls: blocksControls)
        let token = generation
        Task { [weak self] in
            guard let self, isCurrent(token) else { return }
            defer { finishOperation(token: token) }
            do {
                let page = try await client.history(taskID: taskID, cursor: cursor)
                guard
                    acceptHistoryPageIfCurrent(
                        page, cursor: cursor, token: token, historyToken: historyToken, taskID: taskID)
                else { return }
            } catch {
                guard historyRequestIsCurrent(token: token, historyToken: historyToken, taskID: taskID) else { return }
                history.unavailable = true
                history.error = error.localizedDescription
                if let failure = error as? BridgeFailure, historyFailureRequiresRecovery(failure) {
                    recordConnectionFailure(error)
                }
            }
        }
    }

    private func schedulePolling() {
        cancelPolling()
        guard running, !sleeping, !isBusy, !connection.protocolBlocked else { return }
        let interval = TrackerPollingPolicy.interval(visible: uiVisible, failures: connection.failures)
        let token = pollGeneration
        pollTimer = scheduler.schedule(after: interval, repeating: false, tolerance: min(5, interval * 0.2)) {
            [weak self] in
            guard let self, token == pollGeneration, running, !sleeping, !isBusy else { return }
            beginRefresh()
        }
    }

    private func updateDisplayTimer() {
        let needed = running && !sleeping && uiVisible && active != nil
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
            if hasReportClient(), dailyTotals.updateDay(at: clock.now), !isBusy {
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
        guard reports != nil, running, !sleeping, let day = dailyTotals.day else { return }
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
        if uiVisible && !previouslyVisible && !sleeping {
            refresh()
        } else if uiVisible != previouslyVisible {
            schedulePolling()
        }
        publish()
    }

    private func task(withID id: String, in tasks: [TaskItem]) -> TaskItem? {
        for task in tasks {
            if task.id == id { return task }
        }
        return nil
    }

    private func hasTask(withID id: String, archived: Bool, in tasks: [TaskItem]) -> Bool {
        for task in tasks {
            if task.id == id && task.archived == archived { return true }
        }
        return false
    }

    private func containsTask(withID id: String, in tasks: [TaskItem]) -> Bool {
        task(withID: id, in: tasks) != nil
    }

    private func worklog(withID id: String, in worklogs: [WorklogItem]) -> WorklogItem? {
        for worklog in worklogs {
            if worklog.id == id { return worklog }
        }
        return nil
    }

    private func taskIDComesBefore(_ left: String, _ right: String) -> Bool { left < right }

    private func stopPrimaryAction(worklogID: String) -> MenuPrimaryAction {
        canStopTracking ? .stop(worklogID: worklogID) : .disabled
    }

    private func rejectConnection(_ message: String) -> Bool {
        connection.message = message
        publish()
        return false
    }

    private func finishConnection(_ resources: TaskListResources, settings: ConnectionSettings) -> Bool {
        automation.cancel()
        pendingRefresh = false
        connection.settings = settings
        dailyTotals.clear(at: clock.now)
        reportResource = nil
        settingsRepository.save(settings)
        catalog.resetSelections()
        rename.reset()
        archiving.reset()
        history.request(selectedTaskID: nil)
        tracking.error = nil
        acceptTaskList(resources)
        return true
    }

    private func acceptTaskListIfCurrent(_ resources: TaskListResources, token: Int) -> Bool {
        guard isCurrent(token) else { return false }
        acceptTaskList(resources)
        return true
    }

    private func retainRenameIntent(uncertain: Bool, commandStarted: Bool, confirmed: TaskListResources?) -> Bool {
        uncertain || (commandStarted && confirmed == nil)
    }

    private func requireRenameReview(_ message: String) -> Bool {
        rename.requireReview(message)
        return false
    }

    private func prepareMoveSearch() -> Bool {
        guard !sleeping else {
            move.deferSearch()
            return false
        }
        return true
    }

    private func loadedHistoryPages(for taskID: String) -> Int {
        selectedTaskMatches(taskID) ? (worklogs.count + 49) / 50 : 0
    }

    private func selectedTaskMatches(_ taskID: String) -> Bool { selectedTaskID == taskID }

    private func withinHistoryLimit(_ pagesRead: Int, limit: Int) -> Bool { pagesRead < limit }

    private func hasMoreHistory(_ cursor: String?) -> Bool { cursor != nil }

    private func requireMoveReview(latest: WorklogItem?, message: String) -> Bool {
        move.requireReview(latest: latest, message: message)
        return false
    }

    private func failMoveDestination() -> Bool {
        move.fail(
            BridgeFailure(message: "The destination task is unavailable or archived. Choose another task."),
            retainIntent: false)
        return false
    }

    private func isWorklogChangedFailure(_ failure: BridgeFailure?) -> Bool { failure?.kind == "worklog_changed" }

    private func requireCorrectionReview(latest: WorklogItem?, message: String) -> Bool {
        correction.requireReview(latest: latest, message: message)
        return false
    }

    private func pauseTargetsWorklog(_ action: TrackingAutomationState.Pause, worklog: WorklogItem) -> Bool {
        action.discoverAtStartup || action.expectedWorklogID == worklog.id
    }

    private func pauseTimeIsValid(_ start: Date, occurredAt: Date) -> Bool { start <= occurredAt }

    private func automationIsCurrent(_ token: Int) -> Bool { token == automation.generation && automation.enabled }

    private func canRetainPausedTask(_ result: TrackingCommandResult) -> Bool {
        result.didStop && active == nil && !connection.stale
    }

    private func canResumeAutomation(_ resources: TaskListResources, taskID: String) -> Bool {
        resources.tracking.value == nil && hasTask(withID: taskID, archived: false, in: resources.catalog.value)
    }

    private func prepareAutomationResume(_ resources: TaskListResources, taskID: String) -> Bool {
        guard canResumeAutomation(resources, taskID: taskID) else {
            automation.cancel()
            return false
        }
        return true
    }

    private func trackingControlIsCurrent(_ token: Int) -> Bool {
        !sleeping && token == controlGeneration && connection.confirmed && !connection.stale
    }

    private func startCommandCanApply(taskID: String, expectedActiveID: String?, occurredAt: String) -> Bool {
        active?.id == expectedActiveID && hasTask(withID: taskID, archived: false, in: tasks)
            && active?.taskId != taskID && (active == nil || trackingTimestampIsValid(occurredAt))
    }

    private func stopCommandCanApply(worklogID: String, occurredAt: String) -> Bool {
        active?.id == worklogID && trackingTimestampIsValid(occurredAt)
    }

    private func historyRequestIsCurrent(token: Int, historyToken: Int, taskID: String) -> Bool {
        isCurrent(token) && historyToken == history.generation && taskID == selectedTaskID
    }

    private func acceptHistoryPageIfCurrent(
        _ page: HistoryPage, cursor: String?, token: Int, historyToken: Int, taskID: String
    ) -> Bool {
        guard historyRequestIsCurrent(token: token, historyToken: historyToken, taskID: taskID) else { return false }
        history.accept(page, cursor: cursor)
        page.worklogs.forEach(observeHistoryWorklog)
        return true
    }

    private func observeHistoryWorklog(_ worklog: WorklogItem) {
        correction.observe(worklog)
        move.observe(worklog)
    }

    private func historyFailureRequiresRecovery(_ failure: BridgeFailure) -> Bool {
        failure.kind == "unavailable" || failure.kind == "protocol" || failure.requiresRefresh
    }

    private func displayTickIsCurrent(_ token: Int) -> Bool {
        token == displayGeneration && running && !sleeping && uiVisible && active != nil
    }

    private func rolloverTickIsCurrent(_ token: Int) -> Bool { token == rolloverGeneration && running && !sleeping }

    private func hasReportClient() -> Bool { reports != nil }

    private func publish() { onChange?() }
}
