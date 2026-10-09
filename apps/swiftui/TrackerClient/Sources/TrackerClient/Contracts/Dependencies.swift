import Foundation

public protocol ReportClient: Sendable {
    func refreshDailyTotals(settings: ConnectionSettings, start: String, end: String, now: String)
        async throws -> DailyTotalsResources
    func report(settings: ConnectionSettings, start: String, end: String, now: String) async throws -> TrackerReport
    func refreshTaskListWithTotals(settings: ConnectionSettings, start: String, end: String, now: String)
        async throws -> TaskListTotalsRefresh
}

public protocol TrackerClient: Sendable {
    func readTaskCatalog() async throws -> TaskCatalogObservation
    func readTracking() async throws -> TrackingObservation
    func openConfigured(_ settings: ConnectionSettings) async throws -> TaskListResources
    func test(_ settings: ConnectionSettings) async throws
    func connect(_ settings: ConnectionSettings) async throws -> TaskListResources
    func refreshTaskList(settings: ConnectionSettings) async throws -> TaskListResources
    func refreshTaskList() async throws -> TaskListResources
    func createTask(name: String, occurredAt: String) async throws -> TaskCreationResult
    func previewInactiveTasks(inactiveDays: Int, asOf: String) async throws -> InactiveTaskPreview
    func archiveInactiveTasks(preview: InactiveTaskPreview) async throws -> InactiveTaskArchiveResult
    func archiveTask(taskID: String, occurredAt: String) async throws -> TaskCommandResult
    func unarchiveTask(taskID: String, occurredAt: String) async throws -> TaskCommandResult
    func renameTask(taskID: String, name: String, occurredAt: String) async throws -> TaskCommandResult
    func correctWorklog(
        expected: WorklogItem, replacementStart: String, replacementEnd: String?,
        occurredAt: String
    ) async throws -> WorklogCorrectionResult
    func moveCandidates(sourceTaskID: String, query: String) async throws -> [WorklogMoveCandidate]
    func moveWorklog(expected: WorklogItem, destinationTaskID: String) async throws -> WorklogMoveResult
    func startTracking(taskID: String, expectedActiveID: String?, occurredAt: String) async throws
        -> TrackingCommandResult
    func stopTracking(worklogID: String, occurredAt: String) async throws -> TrackingCommandResult
    func pauseTracking(worklogID: String, occurredAt: String) async throws -> TrackingPauseResult
    func resumeTracking(taskID: String, occurredAt: String) async throws -> TrackingCommandResult
    func history(taskID: String, cursor: String?) async throws -> HistoryPage
}

@MainActor
public protocol TrackerClock {
    var now: Date { get }
    var uptime: TimeInterval { get }
}

public protocol TrackerCancellation {
    func cancel()
}

@MainActor
public protocol TrackerScheduler {
    func schedule(
        after: TimeInterval, repeating: Bool, tolerance: TimeInterval,
        action: @escaping @MainActor () -> Void
    ) -> any TrackerCancellation
}

@MainActor
public protocol ConnectionSettingsRepository {
    func load() -> ConnectionSettings?
    func save(_ settings: ConnectionSettings)
}

@MainActor
public protocol TrackingPreferencesRepository {
    func load() -> TrackingPreferences
    func save(_ preferences: TrackingPreferences)
}

@MainActor
public protocol LastTrackedTaskRepository {
    func load(for settings: ConnectionSettings) -> String?
    func save(taskID: String, for settings: ConnectionSettings)
}
