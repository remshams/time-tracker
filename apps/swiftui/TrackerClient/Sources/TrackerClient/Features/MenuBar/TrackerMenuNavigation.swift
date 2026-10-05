import Foundation

public enum TrackerMenuAction: Hashable {
    case stop(worklogID: String)
    case task(id: String)
    case otherTasks
    case openTracker
    case quit
}

public extension TrackerMenuContent {
    func displayedActions(otherTasksExpanded: Bool) -> [TrackerMenuAction] {
        var actions: [TrackerMenuAction] = []
        if let id = activeWorklogID { actions.append(.stop(worklogID: id)) }
        actions += todayTasks.map { .task(id: $0.id) }
        if !otherTasks.isEmpty {
            actions.append(.otherTasks)
            if otherTasksExpanded { actions += otherTasks.map { .task(id: $0.id) } }
        }
        return actions + [.openTracker, .quit]
    }

    func availableActions(otherTasksExpanded: Bool) -> [TrackerMenuAction] {
        var actions: [TrackerMenuAction] = []
        if let id = activeWorklogID, canStopTracking { actions.append(.stop(worklogID: id)) }
        actions += todayTasks.filter { $0.canStart || ($0.isRunning && !isStale) }
            .map { .task(id: $0.id) }
        if !otherTasks.isEmpty {
            actions.append(.otherTasks)
            if otherTasksExpanded {
                actions += otherTasks.filter(\.canStart).map { .task(id: $0.id) }
            }
        }
        actions += [.openTracker, .quit]
        return actions
    }
}

public struct TrackerMenuNavigation {
    public private(set) var actions: [TrackerMenuAction] = []
    public private(set) var selection: TrackerMenuAction?
    private var displayedActions: [TrackerMenuAction] = []

    public init() {}

    public mutating func update(actions next: [TrackerMenuAction],
                                displayedActions nextDisplayed: [TrackerMenuAction]? = nil) {
        let oldIndex = selection.flatMap { displayedActions.firstIndex(of: $0) } ?? 0
        displayedActions = nextDisplayed ?? next
        let displayed = Set(displayedActions)
        actions = next.filter { displayed.contains($0) }
        if let selection, displayed.contains(selection) { return }
        let enabled = Set(actions)
        let index = min(oldIndex, displayedActions.count)
        selection = displayedActions.dropFirst(index).first { enabled.contains($0) }
            ?? displayedActions.prefix(index).last { enabled.contains($0) }
    }

    public mutating func select(_ action: TrackerMenuAction) {
        guard actions.contains(action) else { return }
        selection = action
    }

    public mutating func move(forward: Bool) {
        guard !actions.isEmpty else { return }
        guard let selection, let index = displayedActions.firstIndex(of: selection) else {
            self.selection = forward ? actions.first : actions.last
            return
        }
        let enabled = Set(actions)
        for distance in 1...displayedActions.count {
            let offset = forward ? distance : displayedActions.count - distance
            let candidate = displayedActions[(index + offset) % displayedActions.count]
            if enabled.contains(candidate) {
                self.selection = candidate
                return
            }
        }
    }

    public mutating func selectBoundary(first: Bool) {
        selection = first ? actions.first : actions.last
    }
}
