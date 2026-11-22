import Foundation

/// The row the Quick Track dialog's arrow keys and Return act on.
public enum QuickTrackHighlight: Equatable, Sendable {
    /// The running task at the top. Return stops it.
    case header
    /// A task in the list. Return starts it.
    case task(String)
    /// The "Create and start" row. Return creates a task named after the search text and starts it.
    case create
}

/// A task row in the Quick Track list.
public struct QuickTrackTask: Identifiable, Equatable, Sendable {
    public let task: TaskItem
    /// The task's time today, or `nil` when it has none.
    public let durationText: MenuDurationText?
    public let canStart: Bool
    public var id: String { task.id }
}

/// The Quick Track dialog's top row: the running task or "Not tracking", and today's total.
public struct QuickTrackHeader: Equatable, Sendable {
    /// The running task's ID, for its color, or `nil` while nothing runs.
    public let taskID: String?
    public let title: String
    /// The running worklog's time, or `nil` while nothing runs.
    public let elapsedText: MenuDurationText?
    public let isRunning: Bool
    public let canStop: Bool
    public let totalText: MenuDurationText
    public let totalsExplanation: String
}

/// What the Quick Track dialog shows, computed from the menu bar's store values and the latest search result.
public struct QuickTrackContent: Equatable, Sendable {
    public let header: QuickTrackHeader
    /// Connection, stale, auto-pause, tracking and search messages, shown under the search field.
    public let statusLines: [String]
    public let tasks: [QuickTrackTask]
    /// The name the "Create and start" row creates, or `nil` when the row doesn't show: the search text is blank, or
    /// an active task already has this name, ignoring case.
    public let createName: String?
    /// Whether the create row can run now. It can't while the state is stale or a create request runs.
    public let canCreate: Bool
    public let highlight: QuickTrackHighlight?

    /// - Parameters:
    ///   - query: the search text. An empty or blank query lists today's tasks first.
    ///   - results: the task IDs of the latest search, in rank order.
    ///   - highlight: the highlight the user picked with the arrow keys, if any.
    ///   - messages: search and create errors to show under the search field.
    @MainActor
    init(
        _ inputs: TrackerMenuInputs, query: String, results: [String], highlight: QuickTrackHighlight?,
        messages: [String] = [], isCreating: Bool = false
    ) {
        let isRunning = inputs.active != nil
        header = QuickTrackHeader(
            taskID: inputs.active?.taskId, title: isRunning ? inputs.runningTaskName : "Not tracking",
            elapsedText: isRunning ? inputs.elapsedText : nil, isRunning: isRunning,
            canStop: isRunning && inputs.canStopTracking && !inputs.isStale,
            totalText: inputs.dayTotalText, totalsExplanation: inputs.totalsExplanation)
        var lines: [String] = []
        if inputs.isStale {
            lines.append(inputs.connectionStatusText)
            lines.append("Showing last confirmed state")
        }
        lines.append(contentsOf: [inputs.autoPauseStatusText, inputs.trackingError].compactMap { $0 })
        lines.append(contentsOf: messages)
        statusLines = lines
        let tasksByID = Dictionary(inputs.tasks.map { ($0.id, $0) }, uniquingKeysWith: { first, _ in first })
        let runningTaskID = inputs.active?.taskId
        let matches = results.compactMap { tasksByID[$0] }.filter { !$0.archived && $0.id != runningTaskID }
        let workedToday = { (task: TaskItem) in (inputs.taskDurations[task.id] ?? 0) > 0 }
        // Without a query, today's tasks come first. Both groups keep the search's rank order.
        let ordered =
            QuickTrackContent.isBlank(query)
            ? matches.filter(workedToday) + matches.filter { !workedToday($0) } : matches
        tasks = ordered.map { task in
            QuickTrackTask(
                task: task, durationText: workedToday(task) ? inputs.durationText(taskID: task.id) : nil,
                canStart: !inputs.isStale && inputs.startableTaskIDs.contains(task.id))
        }
        let name = TaskNameEditingPolicy.normalized(query)
        let taken = inputs.tasks.contains { !$0.archived && $0.name.lowercased() == name.lowercased() }
        createName = name.isEmpty || taken ? nil : name
        canCreate = !inputs.isStale && !isCreating
        self.highlight = QuickTrackContent.resolve(
            highlight, query: query, tasks: tasks, createName: createName, headerSelectable: header.isRunning)
    }

    /// Whether `query` is empty once leading and trailing whitespace is removed.
    public static func isBlank(_ query: String) -> Bool { TaskNameEditingPolicy.normalized(query).isEmpty }

    /// Keeps a highlight that still exists. While the user types, the first match is highlighted.
    private static func resolve(
        _ highlight: QuickTrackHighlight?, query: String, tasks: [QuickTrackTask], createName: String?,
        headerSelectable: Bool
    ) -> QuickTrackHighlight? {
        switch highlight {
        case .header where headerSelectable: return .header
        case .task(let id) where tasks.contains(where: { $0.id == id }): return .task(id)
        case .create where createName != nil: return .create
        default:
            guard !isBlank(query) else { return nil }
            // The create row gets the highlight only when it's the only row.
            if let first = tasks.first { return .task(first.id) }
            return createName == nil ? nil : .create
        }
    }

    /// Whether `row` is on screen in this content.
    func hasRow(_ row: QuickTrackHighlight) -> Bool {
        switch row {
        case .header: return header.isRunning
        case .task(let id): return tasks.contains { $0.id == id }
        case .create: return createName != nil
        }
    }

    /// The highlight one row below or above the current one. The header sits above the first task while a task
    /// runs. Moving past either end keeps the highlight where it is.
    func movingHighlight(down: Bool) -> QuickTrackHighlight? {
        var rows: [QuickTrackHighlight] = header.isRunning ? [.header] : []
        rows += tasks.map { .task($0.id) }
        if createName != nil { rows.append(.create) }
        guard let current = highlight, let index = rows.firstIndex(of: current) else {
            // With nothing highlighted, down picks the first task and up the header.
            if down { return tasks.first.map { .task($0.id) } }
            return header.isRunning ? .header : nil
        }
        let next = down ? index + 1 : index - 1
        return rows.indices.contains(next) ? rows[next] : current
    }
}
