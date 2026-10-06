public enum MenuTaskTrackingAction: Equatable, Sendable {
    case start(String)
    case stop(String)
}

public extension TrackerMenuTask {
    func trackingAction(in content: TrackerMenuContent) -> MenuTaskTrackingAction? {
        guard !content.isStale else { return nil }
        if isRunning {
            guard content.canStopTracking, let worklogID = content.activeWorklogID else { return nil }
            return .stop(worklogID)
        }
        guard canStart, !task.archived else { return nil }
        return .start(id)
    }
}
