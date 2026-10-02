import Foundation

struct TaskItem: Decodable, Identifiable {
    let id: String
    let name: String
    let archived: Bool
    let latestStart: String?
}

struct WorklogItem: Decodable, Identifiable {
    let id: String
    let taskId: String
    let start: String
    let end: String?
}

struct TrackerSnapshot: Decodable {
    let tasks: [TaskItem]
    let active: WorklogItem?
}

struct HistoryPage: Decodable {
    let worklogs: [WorklogItem]
    let nextCursor: String?
    let reset: Bool
}

private struct BridgeEnvelope<Value: Decodable>: Decodable {
    let data: Value?
    let error: String?
}

private struct BridgeFailure: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

private final class RustBridge {
    private let handle: OpaquePointer

    init() throws {
        var error: UnsafeMutablePointer<CChar>?
        guard let handle = tt_bridge_open(&error) else {
            let message = error.map { String(cString: $0) } ?? "Could not open the database."
            if let error { tt_bridge_string_free(error) }
            throw BridgeFailure(message: message)
        }
        self.handle = handle
    }

    deinit { tt_bridge_close(handle) }

    private func decode<Value: Decodable>(_ pointer: UnsafeMutablePointer<CChar>?) throws -> Value {
        guard let pointer else { throw BridgeFailure(message: "The database bridge returned no data.") }
        defer { tt_bridge_string_free(pointer) }
        let bytes = Data(String(cString: pointer).utf8)
        let envelope = try JSONDecoder().decode(BridgeEnvelope<Value>.self, from: bytes)
        if let error = envelope.error { throw BridgeFailure(message: error) }
        guard let data = envelope.data else {
            throw BridgeFailure(message: "The database bridge returned an empty result.")
        }
        return data
    }

    func snapshot(refresh: Bool) throws -> TrackerSnapshot {
        try decode(tt_bridge_snapshot(handle, refresh))
    }

    func history(taskID: String, cursor: String?) throws -> HistoryPage {
        try taskID.withCString { taskPointer in
            if let cursor {
                return try cursor.withCString { cursorPointer in
                    try decode(tt_bridge_history(handle, taskPointer, cursorPointer))
                }
            }
            return try decode(tt_bridge_history(handle, taskPointer, nil))
        }
    }
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
    @Published private(set) var historyUnavailable = false
    @Published private(set) var selectedTaskID: String?
    @Published private(set) var tab: TaskTab = .active
    @Published private(set) var now = Date()

    private var bridge: RustBridge?
    private var activeSelection: String?
    private var archivedSelection: String?
    private var timerBase: TimeInterval = 0
    private var timerAnchor: TimeInterval = 0
    private var timer: Timer?

    init() {
        do {
            let bridge = try RustBridge()
            self.bridge = bridge
            apply(try bridge.snapshot(refresh: false))
        } catch {
            self.error = error.localizedDescription
        }
        timer = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in self?.tick() }
        }
    }

    deinit { timer?.invalidate() }

    var visibleTasks: [TaskItem] { tasks.filter { $0.archived == (tab == .archived) } }
    var selectedTask: TaskItem? { tasks.first { $0.id == selectedTaskID } }
    var hasMoreHistory: Bool { nextCursor != nil }
    var runningTaskName: String {
        guard let active else { return "No timer running" }
        return tasks.first { $0.id == active.taskId }?.name ?? "Unknown task"
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
        loadHistory()
    }

    func select(_ taskID: String?) {
        guard let taskID, visibleTasks.contains(where: { $0.id == taskID }) else { return }
        guard selectedTaskID != taskID else { return }
        selectedTaskID = taskID
        rememberSelection()
        loadHistory()
    }

    func tick() {
        now = Date()
        refresh()
    }

    func refresh() {
        guard let bridge else { return }
        do {
            apply(try bridge.snapshot(refresh: true))
        } catch {
            self.error = error.localizedDescription
        }
    }

    func retryHistory() { loadHistory() }

    func loadOlder() {
        guard let bridge, let taskID = selectedTaskID, let nextCursor else { return }
        do {
            let page = try bridge.history(taskID: taskID, cursor: nextCursor)
            if page.reset { worklogs = page.worklogs }
            else { worklogs.append(contentsOf: page.worklogs) }
            self.nextCursor = page.nextCursor
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func apply(_ snapshot: TrackerSnapshot) {
        let previousTask = selectedTaskID
        let previousLatest = selectedTask?.latestStart
        let previousActive = active
        tasks = snapshot.tasks
        active = snapshot.active

        if previousActive?.id != active?.id || previousActive?.start != active?.start {
            timerBase = max(0, Date().timeIntervalSince(timestamp(active?.start) ?? Date()))
            timerAnchor = ProcessInfo.processInfo.systemUptime
        }

        if let selectedTaskID, !visibleTasks.contains(where: { $0.id == selectedTaskID }) {
            self.selectedTaskID = firstTaskID(in: tab)
        } else if selectedTaskID == nil {
            selectedTaskID = firstTaskID(in: tab)
        }
        rememberSelection()

        let selectedActiveChanged =
            (previousActive?.taskId == selectedTaskID || active?.taskId == selectedTaskID) &&
            (previousActive?.id != active?.id || previousActive?.start != active?.start)
        if previousTask != selectedTaskID || previousLatest != selectedTask?.latestStart ||
            selectedActiveChanged {
            loadHistory()
        } else if !historyUnavailable {
            error = nil
        }
    }

    private func loadHistory() {
        worklogs = []
        nextCursor = nil
        historyUnavailable = false
        error = nil
        guard let taskID = selectedTaskID, let bridge else { return }
        do {
            let page = try bridge.history(taskID: taskID, cursor: nil)
            worklogs = page.worklogs
            nextCursor = page.nextCursor
        } catch {
            historyUnavailable = true
            self.error = error.localizedDescription
        }
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

func clockDuration(_ seconds: TimeInterval) -> String {
    let total = Int(max(0, seconds))
    return String(format: "%02d:%02d:%02d", total / 3600, total / 60 % 60, total % 60)
}
