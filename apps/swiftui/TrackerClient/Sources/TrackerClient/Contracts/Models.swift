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
    public let rows: [TaskReportTotal]
    public let revision: String?
    public let now: String?

    public init(rows: [TaskReportTotal], revision: String? = nil, now: String? = nil) {
        self.rows = rows
        self.revision = revision
        self.now = now
    }
}

public struct ResourceObservation<Value: Decodable & Equatable & Sendable>: Decodable, Equatable, Sendable {
    public let value: Value
    public let revision: String?

    public init(value: Value, revision: String? = nil) {
        self.value = value
        self.revision = revision
    }

    private enum CodingKeys: String, CodingKey { case value, revision }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        value = try container.decode(Value.self, forKey: .value)
        revision = try container.decodeIfPresent(String.self, forKey: .revision)
    }
}

public typealias TaskCatalogObservation = ResourceObservation<[TaskItem]>
public typealias TrackingObservation = ResourceObservation<WorklogItem?>

public struct TaskListResources: Equatable, Sendable {
    public let catalog: TaskCatalogObservation
    public let tracking: TrackingObservation

    public init(catalog: TaskCatalogObservation, tracking: TrackingObservation) {
        self.catalog = catalog
        self.tracking = tracking
    }
}

public struct DailyTotalsResources: Equatable, Sendable {
    public let report: TrackerReport
    public let tracking: TrackingObservation

    public init(report: TrackerReport, tracking: TrackingObservation) {
        self.report = report
        self.tracking = tracking
    }
}

public struct TaskListTotalsRefresh: Equatable, Sendable {
    public let taskList: TaskListResources
    public let report: TrackerReport

    public init(taskList: TaskListResources, report: TrackerReport) {
        self.taskList = taskList
        self.report = report
    }
}

public struct CommandReceipt: Decodable, Equatable, Sendable {
    public let requestId: String
    public let appliedRevision: String
    public let replayed: Bool

    public init(requestId: String, appliedRevision: String, replayed: Bool) {
        self.requestId = requestId
        self.appliedRevision = appliedRevision
        self.replayed = replayed
    }
}

public struct TaskCommandResult: Decodable, Equatable, Sendable {
    public let task: TaskItem
    public let receipt: CommandReceipt?

    public init(task: TaskItem, receipt: CommandReceipt? = nil) {
        self.task = task
        self.receipt = receipt
    }
}

public struct TrackingCommandResult: Decodable, Equatable, Sendable {
    public let active: WorklogItem?
    public let stopped: WorklogItem?
    public let didStop: Bool
    public let receipt: CommandReceipt?

    public init(
        active: WorklogItem?, stopped: WorklogItem? = nil, didStop: Bool = false,
        receipt: CommandReceipt? = nil
    ) {
        self.active = active
        self.stopped = stopped
        self.didStop = didStop
        self.receipt = receipt
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

    public init(
        message: String, kind: String = "general", uncertain: Bool = false,
        requiresRefresh: Bool = false
    ) {
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

public typealias TrackingPauseResult = TrackingCommandResult
public typealias TaskCreationResult = TaskCommandResult

public struct WorklogCorrectionResult: Decodable, Equatable, Sendable {
    public let worklog: WorklogItem
    public let receipt: CommandReceipt?

    public init(worklog: WorklogItem, receipt: CommandReceipt? = nil) {
        self.worklog = worklog
        self.receipt = receipt
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

public typealias WorklogMoveResult = WorklogCorrectionResult

public struct InactiveTaskPreview: Decodable, Equatable, Sendable {
    public let asOf: String
    public let inactiveDays: Int
    public let tasks: [TaskItem]

    public init(asOf: String, inactiveDays: Int, tasks: [TaskItem]) {
        self.asOf = asOf
        self.inactiveDays = inactiveDays
        self.tasks = tasks
    }
}

public struct InactiveTaskArchiveResult: Decodable, Equatable, Sendable {
    public let archivedCount: Int
    public let receipt: CommandReceipt?

    public init(archivedCount: Int, receipt: CommandReceipt? = nil) {
        self.archivedCount = archivedCount
        self.receipt = receipt
    }
}
