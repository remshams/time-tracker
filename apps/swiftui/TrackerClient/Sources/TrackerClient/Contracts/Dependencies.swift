import Foundation

public protocol ReportClient: Sendable {
    func refreshTaskListWithTotals(settings: ConnectionSettings, start: String, end: String, now: String)
        async throws -> TaskListTotalsRefresh
}

public protocol TrackerClient: Sendable {
    func openConfigured(_ settings: ConnectionSettings) async throws -> TaskListResources
    func test(_ settings: ConnectionSettings) async throws
    func connect(_ settings: ConnectionSettings) async throws -> TaskListResources
    func refreshTaskList(settings: ConnectionSettings) async throws -> TaskListResources
    func createTask(name: String, occurredAt: String) async throws -> TaskCreationResult
    func previewInactiveTasks(inactiveDays: Int, asOf: String) async throws -> InactiveTaskPreview
    func archiveInactiveTasks(preview: InactiveTaskPreview) async throws -> InactiveTaskArchiveResult
    func archiveTask(taskID: String, expectedName: String, occurredAt: String) async throws -> TaskCommandResult
    func unarchiveTask(taskID: String, occurredAt: String) async throws -> TaskCommandResult
    func renameTask(taskID: String, expectedName: String, name: String, occurredAt: String) async throws
        -> TaskCommandResult
    func correctWorklog(
        expected: WorklogItem, replacementStart: String, replacementEnd: String?,
        occurredAt: String
    ) async throws -> WorklogCorrectionResult
    func moveCandidates(sourceTaskID: String, query: String) async throws -> [WorklogMoveCandidate]
    func moveWorklog(expected: WorklogItem, destinationTaskID: String) async throws -> WorklogMoveResult
    func deleteWorklog(expected: WorklogItem) async throws -> WorklogDeletionResult
    func startTracking(taskID: String, expectedActiveID: String?, occurredAt: String) async throws
        -> TrackingCommandResult
    func stopTracking(worklogID: String, occurredAt: String) async throws -> TrackingCommandResult
    func pauseTracking(worklogID: String, occurredAt: String) async throws -> TrackingPauseResult
    func resumeTracking(taskID: String, occurredAt: String) async throws -> TrackingCommandResult
    func history(taskID: String, cursor: String?) async throws -> HistoryPage
}

public protocol TrackerClock {
    var now: Date { get }
    var uptime: TimeInterval { get }
}

public protocol TrackerCancellation {
    func cancel()
}

public protocol TrackerScheduler {
    func schedule(
        after: TimeInterval, repeating: Bool, tolerance: TimeInterval,
        action: @escaping @MainActor () -> Void
    ) -> any TrackerCancellation
}

public protocol ConnectionSettingsRepository {
    func load() -> ConnectionSettings?
    func save(_ settings: ConnectionSettings)
}

public protocol TrackingPreferencesRepository {
    func load() -> TrackingPreferences
    func save(_ preferences: TrackingPreferences)
}

public protocol LastTrackedTaskRepository {
    func load(for settings: ConnectionSettings) -> String?
    func save(taskID: String, for settings: ConnectionSettings)
}
