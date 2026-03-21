import Foundation

struct WorkLogEntry: Identifiable, Equatable, Sendable {
    enum ValidationError: Error, Equatable {
        case endedBeforeStarted
        case updatedBeforeAdded
    }

    let id: UUID
    let taskID: Task.ID
    let description: String?
    let startedAt: Date
    let addedAt: Date
    let endedAt: Date?
    let updatedAt: Date

    var duration: Duration? {
        guard let endedAt else {
            return nil
        }

        return .seconds(endedAt.timeIntervalSince(startedAt))
    }

    init(
        id: UUID = UUID(),
        taskID: Task.ID,
        description: String? = nil,
        startedAt: Date,
        addedAt: Date = .now,
        endedAt: Date? = nil,
        updatedAt: Date = .now
    ) throws {
        if let endedAt, endedAt < startedAt {
            throw ValidationError.endedBeforeStarted
        }

        guard updatedAt >= addedAt else {
            throw ValidationError.updatedBeforeAdded
        }

        self.id = id
        self.taskID = taskID
        self.description = description
        self.startedAt = startedAt
        self.addedAt = addedAt
        self.endedAt = endedAt
        self.updatedAt = updatedAt
    }

    var formattedTimeRange: String {
        let start = startedAt.formatted(date: .omitted, time: .shortened)
        if let endedAt {
            let end = endedAt.formatted(date: .omitted, time: .shortened)
            return "\(start) – \(end)"
        }
        let running = String(localized: "work-log-entry.running", defaultValue: "Running")
        return "\(start) – \(running)"
    }

    var formattedDuration: String {
        guard let duration else {
            return String(localized: "work-log-entry.duration.unknown", defaultValue: "–")
        }
        let totalSeconds = Int(duration.components.seconds)
        let hours = totalSeconds / 3600
        let minutes = (totalSeconds % 3600) / 60
        let seconds = totalSeconds % 60
        if hours > 0 {
            return "\(hours)h \(minutes)m"
        }
        if minutes > 0 {
            return "\(minutes)m"
        }
        return "\(seconds)s"
    }
}
