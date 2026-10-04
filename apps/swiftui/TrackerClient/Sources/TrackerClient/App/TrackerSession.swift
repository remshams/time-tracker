import Foundation

@MainActor
public final class TrackerSession {
    public var onChange: (() -> Void)?
    public private(set) var isBusy = true
    public private(set) var now: Date

    private enum Lifecycle { case idle, running, stopped }
    private var lifecycle: Lifecycle = .idle
    private let client: any TrackerClient
    private let clock: any TrackerClock
    private let scheduler: any TrackerScheduler
    private let settingsRepository: any ConnectionSettingsRepository
    private let connection: ConnectionState
    private let catalog = TaskCatalogState()
    private let tracking = TrackingState()
    private let history = WorklogHistoryState()
    private var displayTimer: (any TrackerCancellation)?
    private var pollTimer: (any TrackerCancellation)?
    private var displayGeneration = 0
    private var pollGeneration = 0
    private var generation = 0
    private var pendingRefresh = false
    private var sleeping = false
    private var windowVisible = false
    private var menuDepth = 0
    private var uiVisible: Bool { windowVisible || menuDepth > 0 }
    private var running: Bool { lifecycle == .running }

    public init(client: any TrackerClient, clock: any TrackerClock,
                scheduler: any TrackerScheduler, settings: any ConnectionSettingsRepository) {
        self.client = client
        self.clock = clock
        self.scheduler = scheduler
        settingsRepository = settings
        connection = ConnectionState(settings: settings.load() ?? .local)
        now = clock.now
    }

    deinit {
        displayTimer?.cancel()
        pollTimer?.cancel()
    }

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

    public func start() {
        guard lifecycle == .idle else { return }
        lifecycle = .running
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
        updateDisplayTimer()
        refresh()
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
            pendingRefresh = false
            connection.settings = normalized
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
        changeTracking { [client] in
            try await client.startTracking(taskID: taskID, expectedActiveID: expectedActiveID, occurredAt: occurredAt)
        }
    }

    public func stopTracking(worklogID: String) {
        guard canStopTracking, active?.id == worklogID else { return }
        let occurredAt = commandTimestamp(clock.now)
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
        if !sleeping {
            if pendingRefresh {
                pendingRefresh = false
                beginRefresh()
            } else if history.pending {
                history.pending = false
                if let taskID = selectedTaskID {
                    loadHistory(taskID: taskID, cursor: nil, historyToken: history.generation)
                } else { schedulePolling() }
            } else { schedulePolling() }
        }
        publish()
    }

    private func beginRefresh() {
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

    private func changeTracking(_ command: @escaping @MainActor () async throws -> TrackerSnapshot) {
        tracking.error = nil
        beginOperation()
        let token = generation
        Task { [weak self] in
            guard let self, isCurrent(token) else { return }
            defer { finishOperation(token: token) }
            do {
                let snapshot = try await command()
                guard isCurrent(token) else { return }
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

    private func acceptSnapshot(_ snapshot: TrackerSnapshot) {
        let retryFailedHistory = connection.stale && history.unavailable
        connection.acceptSnapshot()
        let previousActive = active
        tracking.apply(snapshot.active, clock: clock)
        if catalog.apply(snapshot.tasks, previousActive: previousActive, active: active) {
            requestHistory()
        } else if !history.unavailable { history.error = nil }
        if retryFailedHistory && !history.pending { requestHistory() }
        updateDisplayTimer()
        publish()
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
            publish()
        }
    }

    private func cancelTimers() {
        cancelPolling()
        cancelDisplay()
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
