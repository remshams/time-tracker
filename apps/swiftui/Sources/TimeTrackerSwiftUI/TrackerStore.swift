import AppKit
import Combine
import Foundation

struct TaskItem: Decodable, Identifiable, Equatable, Sendable {
    let id: String
    let name: String
    let archived: Bool
    let latestStart: String?
}

struct WorklogItem: Decodable, Identifiable, Equatable, Sendable {
    let id: String
    let taskId: String
    let start: String
    let end: String?
}

struct TrackerSnapshot: Decodable, Sendable {
    let tasks: [TaskItem]
    let active: WorklogItem?
}

struct HistoryPage: Decodable, Sendable {
    let worklogs: [WorklogItem]
    let nextCursor: String?
    let reset: Bool
}

enum TaskTab: String, CaseIterable, Hashable, Identifiable {
    case active = "Active"
    case archived = "Archived"
    var id: Self { self }
}

@MainActor
final class TrackerStore: ObservableObject {
    @Published private(set) var tasks: [TaskItem] = []
    @Published private(set) var active: WorklogItem?
    @Published private(set) var worklogs: [WorklogItem] = []
    @Published private(set) var nextCursor: String?
    @Published private(set) var error: String?
    @Published private(set) var trackingError: String?
    @Published private(set) var historyUnavailable = false
    @Published private(set) var selectedTaskID: String?
    @Published private(set) var tab: TaskTab = .active
    @Published private(set) var now = Date()
    @Published private(set) var connectionSettings: ConnectionSettings
    @Published private(set) var connectionStatusText = "Connecting"
    @Published private(set) var connectionMessage: String?
    @Published private(set) var isBusy = true
    @Published private(set) var isChangingConnection = false
    @Published private(set) var isStale = true

    private static let settingsKey = "tracker.connection"
    private let worker = TrackerWorker()
    private var activeSelection: String?
    private var archivedSelection: String?
    private var timerBase: TimeInterval = 0
    private var timerAnchor: TimeInterval = 0
    private var displayTimer: Timer?
    private var pollTimer: Timer?
    private var observers: [(NotificationCenter, NSObjectProtocol)] = []
    private var generation = 0
    private var historyGeneration = 0
    private var pendingHistory = false
    private var pendingRefresh = false
    private var hasConfirmedSnapshot = false
    private var refreshFailures = 0
    private var protocolBlocked = false
    private var sleeping = false
    private var windowVisible = false
    private var menuDepth = 0
    private var uiVisible: Bool { windowVisible || menuDepth > 0 }

    init() {
        let saved = UserDefaults.standard.data(forKey: Self.settingsKey)
        connectionSettings = saved.flatMap { try? JSONDecoder().decode(ConnectionSettings.self, from: $0) } ?? .local
        installLifecycleObservers()
        updateVisibility()
        let settings = connectionSettings
        Task { [weak self] in
            guard let self else { return }
            do {
                let snapshot = try await worker.openConfigured(settings)
                acceptSnapshot(snapshot)
            } catch {
                // The worker retains an opened remote handle even when the server is offline.
                recordConnectionFailure(error)
            }
            finishOperation()
        }
    }

    deinit {
        displayTimer?.invalidate()
        pollTimer?.invalidate()
        for (center, observer) in observers { center.removeObserver(observer) }
    }

    var visibleTasks: [TaskItem] { tasks.filter { $0.archived == (tab == .archived) } }
    var selectedTask: TaskItem? { tasks.first { $0.id == selectedTaskID } }
    var hasMoreHistory: Bool { nextCursor != nil }
    var canStartSelectedTask: Bool {
        guard !isBusy, !isStale, hasConfirmedSnapshot, let task = selectedTask else { return false }
        return !task.archived && active?.taskId != task.id
    }
    var canStopTracking: Bool { !isBusy && !isStale && hasConfirmedSnapshot && active != nil }
    var runningTaskName: String {
        guard hasConfirmedSnapshot else { return "Waiting for tracker state" }
        guard let active else { return "No timer running" }
        return tasks.first { $0.id == active.taskId }?.name ?? "Unknown task"
    }
    var timerDisplayText: String {
        guard hasConfirmedSnapshot else { return "Unavailable" }
        return elapsed.map(clockDuration) ?? "Idle"
    }
    var elapsed: TimeInterval? {
        guard active != nil else { return nil }
        return timerBase + max(0, ProcessInfo.processInfo.systemUptime - timerAnchor)
    }

    func changeTab(_ newTab: TaskTab) {
        guard tab != newTab else { return }
        tab = newTab
        selectedTaskID = selection(for: newTab) ?? firstTaskID(in: newTab)
        rememberSelection()
        requestHistory()
    }

    func select(_ taskID: String?) {
        guard let taskID, visibleTasks.contains(where: { $0.id == taskID }), selectedTaskID != taskID else { return }
        selectedTaskID = taskID
        rememberSelection()
        requestHistory()
    }

    func refresh() {
        guard !sleeping else { return }
        protocolBlocked = false
        if isBusy { pendingRefresh = true; return }
        beginRefresh()
    }

    func retryHistory() { requestHistory() }

    func testConnection(_ settings: ConnectionSettings) async throws {
        guard !isBusy else { throw BridgeFailure(message: "Wait for the current request to finish.") }
        beginOperation()
        defer { finishOperation() }
        try await worker.test(normalized(settings))
    }

    func connect(_ settings: ConnectionSettings) async -> Bool {
        guard !isBusy else {
            connectionMessage = "Wait for the current request to finish before changing connections."
            return false
        }
        beginOperation()
        isChangingConnection = true
        defer { isChangingConnection = false; finishOperation() }
        do {
            let normalized = try normalized(settings)
            let snapshot = try await worker.connect(normalized)
            generation += 1
            historyGeneration += 1
            pendingRefresh = false
            pendingHistory = false
            connectionSettings = normalized
            if let encoded = try? JSONEncoder().encode(normalized) {
                UserDefaults.standard.set(encoded, forKey: Self.settingsKey)
            }
            activeSelection = nil
            archivedSelection = nil
            selectedTaskID = nil
            worklogs = []
            nextCursor = nil
            trackingError = nil
            error = nil
            historyUnavailable = false
            acceptSnapshot(snapshot)
            return true
        } catch {
            connectionMessage = "Connection unchanged. \(error.localizedDescription)"
            return false
        }
    }

    func startTracking(taskID: String) {
        guard !isBusy, !isStale, hasConfirmedSnapshot,
              let task = tasks.first(where: { $0.id == taskID }), !task.archived,
              active?.taskId != taskID else { return }
        let occurredAt = commandTimestamp(Date())
        let expectedActiveID = active?.id
        changeTracking { [worker] in
            try await worker.startTracking(taskID: taskID, expectedActiveID: expectedActiveID, occurredAt: occurredAt)
        }
    }

    func stopTracking(worklogID: String) {
        guard canStopTracking, active?.id == worklogID else { return }
        let occurredAt = commandTimestamp(Date())
        changeTracking { [worker] in try await worker.stopTracking(worklogID: worklogID, occurredAt: occurredAt) }
    }

    func dismissTrackingError() { trackingError = nil }

    func loadOlder() {
        guard !isBusy, let taskID = selectedTaskID, let cursor = nextCursor else { return }
        loadHistory(taskID: taskID, cursor: cursor, historyToken: historyGeneration)
    }

    private func normalized(_ settings: ConnectionSettings) throws -> ConnectionSettings {
        let url = settings.serverURL.trimmingCharacters(in: .whitespacesAndNewlines)
        if settings.mode == .server && url.isEmpty { throw BridgeFailure(message: "Enter a server URL.") }
        return ConnectionSettings(mode: settings.mode, serverURL: url)
    }

    private func beginOperation() {
        isBusy = true
        pollTimer?.invalidate()
        pollTimer = nil
    }

    private func finishOperation() {
        isBusy = false
        guard !sleeping else { return }
        if pendingRefresh {
            pendingRefresh = false
            beginRefresh()
        } else if pendingHistory {
            pendingHistory = false
            if let taskID = selectedTaskID {
                loadHistory(taskID: taskID, cursor: nil, historyToken: historyGeneration)
            } else { schedulePolling() }
        } else { schedulePolling() }
    }

    private func beginRefresh() {
        beginOperation()
        let token = generation
        let settings = connectionSettings
        Task { [weak self] in
            guard let self else { return }
            do {
                let snapshot = try await worker.refresh(settings: settings)
                guard token == generation else { finishOperation(); return }
                acceptSnapshot(snapshot)
            } catch {
                guard token == generation else { finishOperation(); return }
                recordConnectionFailure(error)
            }
            finishOperation()
        }
    }

    private func changeTracking(_ command: @escaping () async throws -> TrackerSnapshot) {
        beginOperation()
        trackingError = nil
        let token = generation
        Task { [weak self] in
            guard let self else { return }
            do {
                let snapshot = try await command()
                guard token == generation else { finishOperation(); return }
                acceptSnapshot(snapshot)
            } catch {
                guard token == generation else { finishOperation(); return }
                let message = error.localizedDescription
                isStale = true
                do {
                    // A failed write may already have committed. Confirm before allowing another.
                    acceptSnapshot(try await worker.snapshot())
                } catch { recordConnectionFailure(error) }
                trackingError = message
            }
            finishOperation()
        }
    }

    private func acceptSnapshot(_ snapshot: TrackerSnapshot) {
        let retryFailedHistory = isStale && historyUnavailable
        hasConfirmedSnapshot = true
        isStale = false
        refreshFailures = 0
        protocolBlocked = false
        connectionStatusText = connectionSettings.mode == .local ? "Local database" : "Connected"
        connectionMessage = nil
        apply(snapshot)
        if retryFailedHistory && !pendingHistory { requestHistory() }
        updateDisplayTimer()
    }

    private func recordConnectionFailure(_ failure: Error) {
        isStale = true
        refreshFailures += 1
        let bridgeError = failure as? BridgeFailure
        protocolBlocked = bridgeError?.kind == "protocol"
        connectionStatusText = protocolBlocked ? "Incompatible server" : "Unavailable"
        connectionMessage = failure.localizedDescription
        error = failure.localizedDescription
    }

    private func apply(_ snapshot: TrackerSnapshot) {
        let previousTask = selectedTaskID
        let previousLatest = selectedTask?.latestStart
        let previousActive = active
        if tasks != snapshot.tasks { tasks = snapshot.tasks }
        if active != snapshot.active { active = snapshot.active }
        if previousActive?.id != active?.id || previousActive?.start != active?.start {
            resetElapsedAnchor()
        }
        if let selectedTaskID, !visibleTasks.contains(where: { $0.id == selectedTaskID }) {
            self.selectedTaskID = firstTaskID(in: tab)
        } else if selectedTaskID == nil { selectedTaskID = firstTaskID(in: tab) }
        rememberSelection()
        let selectedActiveChanged =
            (previousActive?.taskId == selectedTaskID || active?.taskId == selectedTaskID) &&
            (previousActive?.id != active?.id || previousActive?.start != active?.start)
        if previousTask != selectedTaskID || previousLatest != selectedTask?.latestStart || selectedActiveChanged {
            requestHistory()
        } else if !historyUnavailable { error = nil }
    }

    private func requestHistory() {
        historyGeneration += 1
        worklogs = []
        nextCursor = nil
        historyUnavailable = false
        error = nil
        pendingHistory = selectedTaskID != nil
        guard !isBusy, !sleeping else { return }
        pendingHistory = false
        if let taskID = selectedTaskID {
            loadHistory(taskID: taskID, cursor: nil, historyToken: historyGeneration)
        }
    }

    private func loadHistory(taskID: String, cursor: String?, historyToken: Int) {
        beginOperation()
        let token = generation
        Task { [weak self] in
            guard let self else { return }
            do {
                let page = try await worker.history(taskID: taskID, cursor: cursor)
                if token == generation && historyToken == historyGeneration && taskID == selectedTaskID {
                    if cursor == nil || page.reset { worklogs = page.worklogs }
                    else { worklogs.append(contentsOf: page.worklogs) }
                    nextCursor = page.nextCursor
                    historyUnavailable = false
                    error = nil
                }
            } catch {
                if token == generation && historyToken == historyGeneration && taskID == selectedTaskID {
                    historyUnavailable = true
                    self.error = error.localizedDescription
                    if let failure = error as? BridgeFailure,
                       failure.kind == "unavailable" || failure.kind == "protocol" || failure.requiresRefresh {
                        recordConnectionFailure(error)
                    }
                }
            }
            finishOperation()
        }
    }

    private func schedulePolling() {
        pollTimer?.invalidate()
        pollTimer = nil
        guard !sleeping, !isBusy, !protocolBlocked else { return }
        let interval = TrackerPollingPolicy.interval(visible: uiVisible, failures: refreshFailures)
        let timer = Timer(timeInterval: interval, repeats: false) { [weak self] _ in
            Task { @MainActor [weak self] in
                guard let self, !sleeping, !isBusy else { return }
                beginRefresh()
            }
        }
        timer.tolerance = min(5, interval * 0.2)
        RunLoop.main.add(timer, forMode: .common)
        pollTimer = timer
    }

    private func resetElapsedAnchor() {
        timerBase = max(0, Date().timeIntervalSince(timestamp(active?.start) ?? Date()))
        timerAnchor = ProcessInfo.processInfo.systemUptime
    }

    private func updateDisplayTimer() {
        let needed = !sleeping && uiVisible && active != nil
        if !needed { displayTimer?.invalidate(); displayTimer = nil; return }
        guard displayTimer == nil else { return }
        now = Date()
        let timer = Timer(timeInterval: 1, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in self?.now = Date() }
        }
        timer.tolerance = 0.2
        RunLoop.main.add(timer, forMode: .common)
        displayTimer = timer
    }

    private func installLifecycleObservers() {
        let center = NotificationCenter.default
        let windowEvents: [Notification.Name] = [
            NSWindow.didBecomeKeyNotification, NSWindow.didResignKeyNotification,
            NSWindow.didChangeOcclusionStateNotification, NSWindow.didMiniaturizeNotification,
            NSWindow.didDeminiaturizeNotification, NSWindow.willCloseNotification,
            NSApplication.didBecomeActiveNotification, NSApplication.didResignActiveNotification,
            NSApplication.didChangeOcclusionStateNotification, NSApplication.didHideNotification,
            NSApplication.didUnhideNotification
        ]
        for name in windowEvents {
            observe(center, name) { store in store.updateVisibility() }
        }
        observe(center, NSMenu.didBeginTrackingNotification) { store in
            let previouslyVisible = store.uiVisible
            store.menuDepth += 1
            store.visibilityChanged(previouslyVisible: previouslyVisible)
        }
        observe(center, NSMenu.didEndTrackingNotification) { store in
            let previouslyVisible = store.uiVisible
            store.menuDepth = max(0, store.menuDepth - 1)
            store.visibilityChanged(previouslyVisible: previouslyVisible)
        }
        let workspace = NSWorkspace.shared.notificationCenter
        observe(workspace, NSWorkspace.willSleepNotification) { store in
            store.sleeping = true
            store.pollTimer?.invalidate()
            store.pollTimer = nil
            store.updateDisplayTimer()
        }
        observe(workspace, NSWorkspace.didWakeNotification) { store in
            store.sleeping = false
            store.resetElapsedAnchor()
            store.updateVisibility()
            store.updateDisplayTimer()
            store.refresh()
        }
    }

    private func observe(_ center: NotificationCenter, _ name: Notification.Name,
                         action: @escaping @MainActor (TrackerStore) -> Void) {
        let observer = center.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
            Task { @MainActor [weak self] in
                guard let self else { return }
                action(self)
            }
        }
        observers.append((center, observer))
    }

    private func updateVisibility() {
        let previouslyVisible = uiVisible
        windowVisible = NSApplication.shared.windows.contains {
            $0.styleMask.contains(.titled) && $0.isVisible && !$0.isMiniaturized &&
                $0.occlusionState.contains(.visible)
        }
        visibilityChanged(previouslyVisible: previouslyVisible)
    }

    private func visibilityChanged(previouslyVisible: Bool) {
        updateDisplayTimer()
        if uiVisible && !previouslyVisible && !sleeping { refresh() }
        else if uiVisible != previouslyVisible { schedulePolling() }
    }

    private func selection(for tab: TaskTab) -> String? {
        let saved = tab == .active ? activeSelection : archivedSelection
        return saved.flatMap { id in tasks.first { $0.id == id && $0.archived == (tab == .archived) }?.id }
    }

    private func firstTaskID(in tab: TaskTab) -> String? {
        tasks.first { $0.archived == (tab == .archived) }?.id
    }

    private func rememberSelection() {
        if tab == .active { activeSelection = selectedTaskID }
        else { archivedSelection = selectedTaskID }
    }
}

private let isoFormatter: ISO8601DateFormatter = {
    let formatter = ISO8601DateFormatter()
    formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    return formatter
}()

func timestamp(_ value: String?) -> Date? {
    value.flatMap { isoFormatter.date(from: $0) }
}

private func commandTimestamp(_ date: Date) -> String { isoFormatter.string(from: date) }

func clockDuration(_ seconds: TimeInterval) -> String {
    let total = Int(max(0, seconds))
    return String(format: "%02d:%02d:%02d", total / 3600, total / 60 % 60, total % 60)
}
