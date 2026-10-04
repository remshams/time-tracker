import Foundation

@MainActor
public final class TrackerSession {
    public var onChange: (() -> Void)?
    public private(set) var isBusy = true
    public private(set) var now: Date

    private enum Lifecycle { case idle, running, stopped }
    private var lifecycle: Lifecycle = .idle
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
    private let history = WorklogHistoryState()
    private let dailyTotals: DailyTotalsState
    private var displayTimer: (any TrackerCancellation)?
    private var pollTimer: (any TrackerCancellation)?
    private var rolloverTimer: (any TrackerCancellation)?
    private var displayGeneration = 0
    private var pollGeneration = 0
    private var rolloverGeneration = 0
    private var generation = 0
    private var pendingRefresh = false
    private var pendingOwnStartTaskID: String?
    private var sleeping = false
    private var windowVisible = false
    private var menuDepth = 0
    private var uiVisible: Bool { windowVisible || menuDepth > 0 }
    private var running: Bool { lifecycle == .running }

    public init(client: any TrackerClient, clock: any TrackerClock,
                scheduler: any TrackerScheduler, settings: any ConnectionSettingsRepository,
                trackingPreferences: (any TrackingPreferencesRepository)? = nil,
                reports: (any ReportClient)? = nil, calendar: Calendar = .autoupdatingCurrent) {
        self.client = client
        self.reports = reports
        dailyTotals = DailyTotalsState(calendar: calendar)
        self.clock = clock
        self.scheduler = scheduler
        settingsRepository = settings
        trackingPreferencesRepository = trackingPreferences
        automation = TrackingAutomationState(preferences: trackingPreferences?.load() ?? TrackingPreferences())
        connection = ConnectionState(settings: settings.load() ?? .local)
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
    public var canStartSelectedTask: Bool {
        guard running, !isBusy, !isStale, connection.confirmed, let task = selectedTask else { return false }
        return !task.archived && active?.taskId != task.id
    }
    public var canStopTracking: Bool {
        running && !isBusy && !isStale && connection.confirmed && active != nil
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
                guard isCurrent(token) else { return }
                acceptSnapshot(snapshot)
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
        history.invalidate()
        automation.cancel()
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
        refresh()
    }

    public func setPauseOnScreenLock(_ enabled: Bool) {
        guard lifecycle != .stopped,
              automation.setEnabled(enabled, at: clock.now, active: active,
                                    ownStartTaskID: pendingOwnStartTaskID, confirmed: connection.confirmed) else { return }
        trackingPreferencesRepository?.save(TrackingPreferences(pauseOnScreenLock: enabled))
        drainAutomation()
        publish()
    }

    public func screenLocked(at date: Date) {
        guard lifecycle != .stopped else { return }
        automation.lock(at: date, active: active, ownStartTaskID: pendingOwnStartTaskID,
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
        guard running, !isBusy else { throw BridgeFailure(message: "Wait for the current request to finish.") }
        let token = generation
        beginOperation()
        defer { finishOperation(token: token) }
        try await client.test(connection.normalized(settings))
    }

    public func connect(_ settings: ConnectionSettings) async -> Bool {
        automation.cancel()
        guard running, !isBusy else {
            if running {
                connection.message = "Wait for the current request to finish before changing connections."
                publish()
            }
            return false
        }
        let token = generation
        connection.changing = true
        beginOperation()
        defer {
            if isCurrent(token) {
                connection.changing = false
                finishOperation(token: token)
            }
        }
        do {
            let normalized = try connection.normalized(settings)
            let snapshot = try await client.connect(normalized)
            guard isCurrent(token) else { return false }
            automation.cancel()
            pendingRefresh = false
            connection.settings = normalized
            dailyTotals.clear(at: clock.now)
            settingsRepository.save(normalized)
            catalog.resetSelections()
            history.request(selectedTaskID: nil)
            tracking.error = nil
            acceptSnapshot(snapshot)
            return true
        } catch {
            guard isCurrent(token) else { return false }
            connection.message = "Connection unchanged. \(error.localizedDescription)"
            return false
        }
    }

    public func startTracking(taskID: String) {
        guard running, !isBusy, !isStale, connection.confirmed,
              let task = tasks.first(where: { $0.id == taskID }), !task.archived,
              active?.taskId != taskID else { return }
        let occurredAt = commandTimestamp(clock.now)
        let expectedActiveID = active?.id
        automation.cancel()
        changeTracking(startTaskID: taskID) { [client] in
            try await client.startTracking(taskID: taskID, expectedActiveID: expectedActiveID, occurredAt: occurredAt)
        }
    }

    public func stopTracking(worklogID: String) {
        guard canStopTracking, active?.id == worklogID else { return }
        let occurredAt = commandTimestamp(clock.now)
        automation.cancel()
        changeTracking { [client] in
            try await client.stopTracking(worklogID: worklogID, occurredAt: occurredAt)
        }
    }

    public func dismissTrackingError() {
        guard lifecycle != .stopped else { return }
        tracking.error = nil
        publish()
    }

    public func loadOlder() {
        guard running, !isBusy, let taskID = selectedTaskID, let cursor = nextCursor else { return }
        loadHistory(taskID: taskID, cursor: cursor, historyToken: history.generation)
    }

    private func isCurrent(_ token: Int) -> Bool { running && token == generation }

    private func beginOperation() {
        isBusy = true
        cancelPolling()
        publish()
    }

    private func finishOperation(token: Int) {
        guard isCurrent(token) else { return }
        isBusy = false
        if drainAutomation() { return }
        if !sleeping {
            if pendingRefresh {
                pendingRefresh = false
                beginRefresh()
            } else if reports != nil && dailyTotals.pending && connection.confirmed && !connection.stale {
                beginReport()
            } else if history.pending {
                history.pending = false
                if let taskID = selectedTaskID {
                    loadHistory(taskID: taskID, cursor: nil, historyToken: history.generation)
                } else { schedulePolling() }
            } else { schedulePolling() }
        }
        publish()
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
                  let pausedTaskID = automation.pausedTaskID {
            automation.resumeAt = nil
            action = nil
            resumeAt = date
            taskID = pausedTaskID
        } else { return false }
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
                let snapshot = try await client.snapshot()
                guard isCurrent(token) else { return }
                acceptSnapshot(snapshot)
                guard automationToken == automation.generation, automation.enabled else { return }
                if let action {
                    guard let worklog = snapshot.active,
                          action.discoverAtStartup || action.expectedWorklogID == worklog.id else { return }
                    guard let start = timestamp(worklog.start) else {
                        tracking.error = "Cannot pause tracking because its start time is invalid."
                        return
                    }
                    guard start <= action.occurredAt else {
                        tracking.error = "Cannot pause tracking before the worklog start time."
                        return
                    }
                    let result = try await client.pauseTracking(worklogID: worklog.id,
                                                                occurredAt: commandTimestamp(action.occurredAt))
                    guard isCurrent(token) else { return }
                    acceptSnapshot(result.snapshot)
                    guard automationToken == automation.generation, automation.enabled else { return }
                    if result.didStop && result.snapshot.active == nil { automation.pausedTaskID = worklog.taskId }
                    else { automation.cancel() }
                } else if let taskID, let resumeAt {
                    guard snapshot.active == nil,
                          snapshot.tasks.contains(where: { $0.id == taskID && !$0.archived }) else {
                        automation.cancel()
                        return
                    }
                    guard !automation.locked, !sleeping else { return }
                    automation.pausedTaskID = nil
                    pendingOwnStartTaskID = taskID
                    defer { pendingOwnStartTaskID = nil }
                    let resumed = try await client.resumeTracking(taskID: taskID,
                                                                  occurredAt: commandTimestamp(resumeAt))
                    guard isCurrent(token) else { return }
                    automation.acknowledgeOwnStart(resumed.active, taskID: taskID)
                    acceptSnapshot(resumed)
                }
            } catch {
                guard isCurrent(token) else { return }
                if automationToken == automation.generation { automation.cancel() }
                let message = error.localizedDescription
                connection.stale = true
                publish()
                do {
                    let snapshot = try await client.snapshot()
                    guard isCurrent(token) else { return }
                    acceptSnapshot(snapshot)
                } catch {
                    guard isCurrent(token) else { return }
                    recordConnectionFailure(error)
                }
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
        beginOperation()
        let token = generation
        let settings = connection.settings
        Task { [weak self] in
            guard let self, isCurrent(token) else { return }
            defer { finishOperation(token: token) }
            do {
                let snapshot = try await client.refresh(settings: settings)
                guard isCurrent(token) else { return }
                acceptSnapshot(snapshot)
            } catch {
                guard isCurrent(token) else { return }
                recordConnectionFailure(error)
            }
        }
    }

    private func changeTracking(startTaskID: String? = nil, _ command: @escaping @MainActor () async throws -> TrackerSnapshot) {
        tracking.error = nil
        pendingOwnStartTaskID = startTaskID
        let token = generation
        beginOperation()
        Task { [weak self] in
            guard let self, isCurrent(token) else { return }
            defer {
                pendingOwnStartTaskID = nil
                finishOperation(token: token)
            }
            do {
                let snapshot = try await command()
                guard isCurrent(token) else { return }
                if let startTaskID { automation.acknowledgeOwnStart(snapshot.active, taskID: startTaskID) }
                acceptSnapshot(snapshot)
            } catch {
                guard isCurrent(token) else { return }
                let message = error.localizedDescription
                connection.stale = true
                publish()
                do {
                    // A failed write may have committed. Confirm before accepting another write.
                    let snapshot = try await client.snapshot()
                    guard isCurrent(token) else { return }
                    acceptSnapshot(snapshot)
                } catch {
                    guard isCurrent(token) else { return }
                    recordConnectionFailure(error)
                }
                tracking.error = message
            }
        }
    }

    private func acceptSnapshot(_ snapshot: TrackerSnapshot, requestReport: Bool = true) {
        dailyTotals.updateDay(at: clock.now)
        let retryFailedHistory = connection.stale && history.unavailable
        connection.acceptSnapshot()
        let previousActive = active
        automation.observeActive(snapshot.active)
        tracking.apply(snapshot.active, clock: clock)
        if reports != nil && requestReport { dailyTotals.invalidate() }
        if catalog.apply(snapshot.tasks, previousActive: previousActive, active: active) {
            requestHistory()
        } else if !history.unavailable { history.error = nil }
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
        beginOperation()
        Task { [weak self] in
            guard let self, isCurrent(token) else { return }
            defer { finishOperation(token: token) }
            do {
                let report = try await reports.report(settings: settings, start: requested.start,
                                                      end: requested.end, now: requested.now)
                guard isCurrent(token) else { return }
                try dailyTotals.validate(report)
                dailyTotals.accept(report, requested: requested, clock: clock)
                acceptSnapshot(report.snapshot, requestReport: false)
            } catch {
                guard isCurrent(token) else { return }
                dailyTotals.fail(error, clock: clock)
                if let failure = error as? BridgeFailure,
                   failure.requiresRefresh || failure.uncertain || failure.kind == "unavailable" || failure.kind == "protocol" {
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
            loadHistory(taskID: taskID, cursor: nil, historyToken: history.generation)
        }
    }

    private func loadHistory(taskID: String, cursor: String?, historyToken: Int) {
        beginOperation()
        let token = generation
        Task { [weak self] in
            guard let self, isCurrent(token) else { return }
            defer { finishOperation(token: token) }
            do {
                let page = try await client.history(taskID: taskID, cursor: cursor)
                guard isCurrent(token), historyToken == history.generation, taskID == selectedTaskID else { return }
                history.accept(page, cursor: cursor)
            } catch {
                guard isCurrent(token), historyToken == history.generation, taskID == selectedTaskID else { return }
                history.unavailable = true
                history.error = error.localizedDescription
                if let failure = error as? BridgeFailure,
                   failure.kind == "unavailable" || failure.kind == "protocol" || failure.requiresRefresh {
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
        pollTimer = scheduler.schedule(after: interval, repeating: false, tolerance: min(5, interval * 0.2)) { [weak self] in
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
            guard let self, token == displayGeneration, running, !sleeping, uiVisible, active != nil else { return }
            now = clock.now
            if reports != nil, dailyTotals.updateDay(at: clock.now), !isBusy {
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
        rolloverTimer = scheduler.schedule(after: max(0.001, day.end.timeIntervalSince(clock.now)),
                                            repeating: false, tolerance: 0) { [weak self] in
            guard let self, token == rolloverGeneration, running, !sleeping else { return }
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
        if uiVisible && !previouslyVisible && !sleeping { refresh() }
        else if uiVisible != previouslyVisible { schedulePolling() }
        publish()
    }

    private func publish() { onChange?() }
}
