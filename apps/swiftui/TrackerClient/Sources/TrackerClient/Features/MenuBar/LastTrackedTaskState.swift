import Foundation

public enum MenuPrimaryAction: Equatable, Sendable {
    case stop(worklogID: String)
    case start(taskID: String)
    case openMenu
    case disabled
}

public extension ConnectionSettings {
    var trackingIdentityKey: String {
        mode == .local ? "local" : "server:\(serverURL.trimmingCharacters(in: .whitespacesAndNewlines))"
    }
}

@MainActor
final class LastTrackedTaskState {
    private let repository: (any LastTrackedTaskRepository)?
    private var remembered: [String: String] = [:]
    private(set) var taskID: String?

    init(repository: (any LastTrackedTaskRepository)?, settings: ConnectionSettings) {
        self.repository = repository
        taskID = repository?.load(for: settings)
        remembered[settings.trackingIdentityKey] = taskID
    }

    func observe(_ snapshot: TaskListResources, settings: ConnectionSettings) {
        let key = settings.trackingIdentityKey
        let stored = remembered[key] ?? repository?.load(for: settings)
        let latest = snapshot.catalog.value.compactMap { task -> (String, Date)? in
            guard let start = timestamp(task.latestStart) else { return nil }
            return (task.id, start)
        }.max(by: lessRecent)
        let idleTaskID: String?
        if let stored {
            if let previous = task(withID: stored, in: snapshot.catalog.value), let latest,
                isNewer(latest.1, than: previous)
            {
                idleTaskID = latest.0
            } else {
                idleTaskID = stored
            }
        } else {
            idleTaskID = latest?.0
        }
        taskID = snapshot.tracking.value?.taskId ?? idleTaskID
        guard let taskID else { return }
        if stored != taskID { repository?.save(taskID: taskID, for: settings) }
        remembered[key] = taskID
    }

    private func lessRecent(_ left: (String, Date), _ right: (String, Date)) -> Bool {
        if sameStart(left.1, right.1) { return largerTaskID(left.0, right.0) }
        return left.1 < right.1
    }

    private func sameStart(_ left: Date, _ right: Date) -> Bool { left == right }

    private func largerTaskID(_ left: String, _ right: String) -> Bool { left > right }

    private func task(withID id: String, in tasks: [TaskItem]) -> TaskItem? {
        for task in tasks {
            if task.id == id { return task }
        }
        return nil
    }

    private func isNewer(_ latest: Date, than previous: TaskItem) -> Bool {
        guard let previousStart = timestamp(previous.latestStart) else { return true }
        return latest > previousStart
    }
}
