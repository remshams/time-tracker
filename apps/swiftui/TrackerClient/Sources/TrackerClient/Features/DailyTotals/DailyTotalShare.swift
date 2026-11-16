import Foundation

/// One task's part of today's total, as the sidebar's bar shows it.
public struct DailyTotalShare: Equatable, Sendable, Identifiable {
    public let taskID: String
    public let name: String
    /// The task's time today in whole seconds.
    public let seconds: Int
    /// The task's part of the time of all shares, from 0 to 1.
    public let fraction: Double

    public var id: String { taskID }

    /// The fraction as a whole percentage, such as 33.
    public var percent: Int { Int((fraction * 100).rounded()) }

    /// The segment's VoiceOver label, such as "Client A, 1 hour, 4 minutes, 33 percent".
    public var accessibilityLabel: String { "\(name), \(spokenDuration(seconds)), \(percent) percent" }

    /// One share for each task with at least a second today, largest first, then by name. Tasks without a duration
    /// are left out, so the shares are empty while today's totals are unavailable.
    static func shares(of tasks: [TaskItem], durations: [String: TimeInterval]) -> [DailyTotalShare] {
        let timed = tasks.compactMap { task -> (TaskItem, Int)? in
            guard let seconds = durations[task.id].map(wholeSeconds), seconds > 0 else { return nil }
            return (task, seconds)
        }
        let sum = Double(timed.reduce(0) { $0 + $1.1 })
        let shares = timed.map {
            DailyTotalShare(taskID: $0.id, name: $0.name, seconds: $1, fraction: Double($1) / sum)
        }
        return shares.sorted { ($1.seconds, $0.name, $0.taskID) < ($0.seconds, $1.name, $1.taskID) }
    }
}
