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

    func observe(_ snapshot: TrackerSnapshot, settings: ConnectionSettings) {
        let key = settings.trackingIdentityKey
        let stored = remembered[key] ?? repository?.load(for: settings)
        let latest = snapshot.tasks.compactMap { task -> (String, Date)? in
            guard let start = timestamp(task.latestStart) else { return nil }
            return (task.id, start)
        }.max {
            if $0.1 == $1.1 { return $0.0 > $1.0 }
            return $0.1 < $1.1
        }
        let idleTaskID: String?
        if let stored {
            if let previous = snapshot.tasks.first(where: { $0.id == stored }), let latest,
                timestamp(previous.latestStart).map({ latest.1 > $0 }) ?? true
            {
                idleTaskID = latest.0
            } else {
                idleTaskID = stored
            }
        } else {
            idleTaskID = latest?.0
        }
        taskID = snapshot.active?.taskId ?? idleTaskID
        guard let taskID else { return }
        if stored != taskID { repository?.save(taskID: taskID, for: settings) }
        remembered[key] = taskID
    }
}
