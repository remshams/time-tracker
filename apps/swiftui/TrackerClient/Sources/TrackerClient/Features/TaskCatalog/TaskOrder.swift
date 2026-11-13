import Foundation

/// Sorts the task list by each task's latest worklog start, or by name only.
extension TaskOrder {
    /// Tasks without logged time follow logged tasks. Equal times fall back to name, then ID.
    func sorted(_ tasks: [TaskItem]) -> [TaskItem] {
        tasks.map { RankedTask(task: $0, loggedAt: loggedAt($0)) }
            .sorted(by: isOrderedBefore)
            .map(\.task)
    }

    private func loggedAt(_ task: TaskItem) -> Date? {
        self == .name ? nil : parseTimestamp(task.latestStart)
    }

    private func isOrderedBefore(_ left: RankedTask, _ right: RankedTask) -> Bool {
        switch (left.loggedAt, right.loggedAt) {
        case (let leftDate?, let rightDate?) where leftDate != rightDate:
            return self == .newestFirst ? leftDate > rightDate : leftDate < rightDate
        case (.some, nil): return true
        case (nil, .some): return false
        default: return Self.isOrderedByName(left.task, right.task)
        }
    }

    private static func isOrderedByName(_ left: TaskItem, _ right: TaskItem) -> Bool {
        if left.name == right.name { return left.id < right.id }
        return left.name < right.name
    }
}

private struct RankedTask {
    let task: TaskItem
    let loggedAt: Date?
}
