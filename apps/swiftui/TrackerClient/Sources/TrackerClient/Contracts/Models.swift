import Foundation

public struct TaskReportTotal: Decodable, Equatable, Sendable {
    public let taskId: String
    public let durationMicroseconds: Int64

    public init(taskId: String, durationMicroseconds: Int64) {
        self.taskId = taskId
        self.durationMicroseconds = durationMicroseconds
    }
}

public struct TrackerReport: Decodable, Equatable, Sendable {
    public let snapshot: TrackerSnapshot
    public let rows: [TaskReportTotal]

    public init(snapshot: TrackerSnapshot, rows: [TaskReportTotal]) {
        self.snapshot = snapshot
        self.rows = rows
    }
}

public enum DailyTotalsStatus: String, Equatable, Sendable {
    case unavailable, loading, current, cached
}

public struct TaskItem: Decodable, Identifiable, Equatable, Sendable {
    public let id: String
    public let name: String
    public let archived: Bool
    public let latestStart: String?

    public init(id: String, name: String, archived: Bool, latestStart: String?) {
        self.id = id
        self.name = name
        self.archived = archived
        self.latestStart = latestStart
    }
}

public struct WorklogItem: Decodable, Identifiable, Equatable, Sendable {
    public let id: String
    public let taskId: String
    public let start: String
    public let end: String?

    public init(id: String, taskId: String, start: String, end: String?) {
        self.id = id
        self.taskId = taskId
        self.start = start
        self.end = end
    }
}

public struct TrackerSnapshot: Decodable, Equatable, Sendable {
    public let tasks: [TaskItem]
    public let active: WorklogItem?

    public init(tasks: [TaskItem], active: WorklogItem?) {
        self.tasks = tasks
        self.active = active
    }
}

public struct HistoryPage: Decodable, Equatable, Sendable {
    public let worklogs: [WorklogItem]
    public let nextCursor: String?
    public let reset: Bool

    public init(worklogs: [WorklogItem], nextCursor: String?, reset: Bool) {
        self.worklogs = worklogs
        self.nextCursor = nextCursor
        self.reset = reset
    }
}

public enum TaskTab: String, CaseIterable, Hashable, Identifiable, Sendable {
    case active = "Active"
    case archived = "Archived"
    public var id: Self { self }
}

public enum ConnectionMode: String, CaseIterable, Codable, Identifiable, Sendable {
    case local
    case server
    public var id: Self { self }
    public var label: String { self == .local ? "Local database" : "Server" }
}

public struct ConnectionSettings: Codable, Equatable, Sendable {
    public var mode: ConnectionMode
    public var serverURL: String
    public static let local = ConnectionSettings(mode: .local, serverURL: "")

    public init(mode: ConnectionMode, serverURL: String) {
        self.mode = mode
        self.serverURL = serverURL
    }
}

public struct BridgeFailure: LocalizedError, Sendable {
    public let message: String
    public var kind: String
    public var uncertain: Bool
    public var requiresRefresh: Bool
    public var errorDescription: String? { message }

    public init(message: String, kind: String = "general", uncertain: Bool = false,
                requiresRefresh: Bool = false) {
        self.message = message
        self.kind = kind
        self.uncertain = uncertain
        self.requiresRefresh = requiresRefresh
    }
}

public struct TrackingPreferences: Codable, Equatable, Sendable {
    public var pauseOnScreenLock: Bool

    public init(pauseOnScreenLock: Bool = false) {
        self.pauseOnScreenLock = pauseOnScreenLock
    }
}

public struct TrackingPauseResult: Decodable, Equatable, Sendable {
    public let snapshot: TrackerSnapshot
    public let didStop: Bool

    public init(snapshot: TrackerSnapshot, didStop: Bool) {
        self.snapshot = snapshot
        self.didStop = didStop
    }
}

public struct TaskCreationResult: Decodable, Equatable, Sendable {
    public let taskId: String
    public let snapshot: TrackerSnapshot

    public init(taskId: String, snapshot: TrackerSnapshot) {
        self.taskId = taskId
        self.snapshot = snapshot
    }
}

public struct WorklogCorrectionResult: Decodable, Equatable, Sendable {
    public let worklog: WorklogItem
    public let snapshot: TrackerSnapshot

    public init(worklog: WorklogItem, snapshot: TrackerSnapshot) {
        self.worklog = worklog
        self.snapshot = snapshot
    }
}

public struct WorklogMoveCandidate: Decodable, Identifiable, Equatable, Sendable {
    public let id: String
    public let name: String

    public init(id: String, name: String) {
        self.id = id
        self.name = name
    }
}

public struct WorklogMoveResult: Decodable, Equatable, Sendable {
    public let worklog: WorklogItem
    public let snapshot: TrackerSnapshot

    public init(worklog: WorklogItem, snapshot: TrackerSnapshot) {
        self.worklog = worklog
        self.snapshot = snapshot
    }
}
