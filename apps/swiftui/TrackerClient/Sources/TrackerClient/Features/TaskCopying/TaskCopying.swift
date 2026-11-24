import Foundation

/// The values the copy actions of the menu bar and Quick Track put on the pasteboard.
///
/// It keeps no state. Each call reads the stores, so the time of a running task is current at the moment of the copy.
public struct TaskCopying: Sendable {
    let catalog: TaskCatalogStore
    let dailyTotals: DailyTotalsStore
    let tracking: TrackingStore

    /// The text `action` copies for `taskID`, or `nil` when the task is unknown or its time today isn't known.
    public func value(_ action: TaskCopyAction, taskID: String) -> String? {
        guard let task = catalog.tasks.first(where: { $0.id == taskID }) else { return nil }
        switch action {
        case .copyName: return task.name
        case .copyExact: return TaskCopyFormatter.exact(dailyDuration(taskID: taskID))
        case .copyRounded: return TaskCopyFormatter.rounded(dailyDuration(taskID: taskID))
        }
    }

    /// Computed from the clock on each read, so copying uses the current time.
    func dailyDuration(taskID: String) -> TimeInterval? {
        dailyTotals.duration(taskID: taskID, active: tracking.active)
    }
}
