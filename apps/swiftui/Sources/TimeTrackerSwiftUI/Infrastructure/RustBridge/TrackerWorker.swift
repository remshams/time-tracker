import Foundation
import TrackerClient

private struct BridgeEnvelope<Value: Decodable>: Decodable {
    let data: Value?
    let error: String?
    let kind: String?
    let uncertain: Bool?
    let requiresRefresh: Bool?
    private enum CodingKeys: String, CodingKey { case data, error, kind, uncertain, requiresRefresh }

    init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        data = values.contains(.data) ? .some(try values.decode(Value.self, forKey: .data)) : nil
        error = try values.decodeIfPresent(String.self, forKey: .error)
        kind = try values.decodeIfPresent(String.self, forKey: .kind)
        uncertain = try values.decodeIfPresent(Bool.self, forKey: .uncertain)
        requiresRefresh = try values.decodeIfPresent(Bool.self, forKey: .requiresRefresh)
    }

}

private final class RustBridge {
    private var handle: OpaquePointer?

    init(settings: ConnectionSettings, localDatabasePath: String?) throws {
        var error: UnsafeMutablePointer<CChar>?
        let opened: OpaquePointer?
        if settings.mode == .server {
            opened = settings.serverURL.withCString { tt_bridge_open_remote($0, &error) }
        } else if let localDatabasePath {
            opened = localDatabasePath.withCString { tt_bridge_open_path($0, &error) }
        } else {
            opened = tt_bridge_open(&error)
        }
        defer { if let error { tt_bridge_string_free(error) } }
        guard let opened else {
            throw BridgeFailure(message: error.map { String(cString: $0) } ?? "Could not open the connection.")
        }
        handle = opened
    }

    deinit { close() }

    func close() {
        guard let handle else { return }
        self.handle = nil
        tt_bridge_close(handle)
    }

    private func decode<Value: Decodable>(
        _ pointer: UnsafeMutablePointer<CChar>?,
        requiresRefreshOnMalformed: Bool = false
    ) throws -> Value {
        func malformed(_ message: String) -> BridgeFailure {
            BridgeFailure(
                message: message, kind: requiresRefreshOnMalformed ? "protocol" : "general",
                requiresRefresh: requiresRefreshOnMalformed)
        }
        guard let pointer else { throw malformed("The tracker bridge returned no data.") }
        defer { tt_bridge_string_free(pointer) }
        let envelope: BridgeEnvelope<Value>
        do {
            envelope = try JSONDecoder().decode(BridgeEnvelope<Value>.self, from: Data(String(cString: pointer).utf8))
        } catch {
            guard requiresRefreshOnMalformed else { throw error }
            throw malformed("The tracker bridge returned an invalid result: \(error.localizedDescription)")
        }
        if let error = envelope.error {
            throw BridgeFailure(
                message: error, kind: envelope.kind ?? "general",
                uncertain: envelope.uncertain ?? false,
                requiresRefresh: envelope.requiresRefresh ?? false)
        }
        guard let data = envelope.data else { throw malformed("The tracker bridge returned an empty result.") }
        return data
    }

    func testConnection() throws {
        struct Compatibility: Decodable { let compatible: Bool }
        let _: Compatibility = try decode(tt_bridge_check_connection(handle))
    }

    func refreshTaskList() throws -> TaskListResources {
        // Selection 3 refreshes tasks and tracking together.
        try requireRefreshed(tt_bridge_refresh_resources(handle, 3))
        return try taskListResources()
    }

    private func requireRefreshed(_ pointer: UnsafeMutablePointer<CChar>?) throws {
        struct Refreshed: Decodable { let refreshed: Bool }
        let _: Refreshed = try decode(pointer)
    }

    private func taskListResources() throws -> TaskListResources {
        let catalog: TaskCatalogObservation = try decode(tt_bridge_tasks(handle))
        let tracking: TrackingObservation = try decode(tt_bridge_tracking(handle))
        return TaskListResources(catalog: catalog, tracking: tracking)
    }

    func refreshTaskListWithTotals(start: String, end: String, now: String) throws -> TaskListTotalsRefresh {
        try start.withCString { start in
            try end.withCString { end in
                try now.withCString { now in
                    try requireRefreshed(tt_bridge_refresh_totals(handle, true, start, end, now))
                }
            }
        }
        let taskList = try taskListResources()
        let report: TrackerReport = try decode(tt_bridge_report_observation(handle))
        return TaskListTotalsRefresh(taskList: taskList, report: report)
    }

    func createTask(name: String, occurredAt: String) throws -> TaskCreationResult {
        guard !name.utf8.contains(0) else {
            throw BridgeFailure(message: "Task names must not contain control characters.")
        }
        return try name.withCString { name in
            try occurredAt.withCString { instant in
                try decode(tt_bridge_create_task_at(handle, name, instant), requiresRefreshOnMalformed: true)
            }
        }
    }

    func renameTask(taskID: String, expectedName: String, name: String, occurredAt: String) throws -> TaskCommandResult
    {
        guard !name.utf8.contains(0), !expectedName.utf8.contains(0) else {
            throw BridgeFailure(message: "Task names must not contain control characters.")
        }
        return try taskID.withCString { task in
            try expectedName.withCString { expected in
                try name.withCString { name in
                    try occurredAt.withCString { instant in
                        try decode(
                            tt_bridge_rename_task_with_expected_name_at(handle, task, expected, name, instant),
                            requiresRefreshOnMalformed: true)
                    }
                }
            }
        }
    }

    func previewInactiveTasks(inactiveDays: Int, asOf: String) throws -> InactiveTaskPreview {
        guard let days = UInt32(exactly: inactiveDays), days > 0, !asOf.utf8.contains(0) else {
            throw BridgeFailure(message: "The archive period or date is invalid.")
        }
        return try asOf.withCString {
            try decode(tt_bridge_preview_inactive_tasks_at(handle, days, $0), requiresRefreshOnMalformed: true)
        }
    }

    func archiveInactiveTasks(preview: InactiveTaskPreview) throws -> InactiveTaskArchiveResult {
        guard let days = UInt32(exactly: preview.inactiveDays), days > 0, !preview.asOf.utf8.contains(0) else {
            throw BridgeFailure(message: "The archive period or date is invalid.")
        }
        return try preview.asOf.withCString {
            try decode(tt_bridge_archive_inactive_tasks_at(handle, days, $0), requiresRefreshOnMalformed: true)
        }
    }

    func archiveTask(taskID: String, expectedName: String, occurredAt: String) throws -> TaskCommandResult {
        guard !expectedName.utf8.contains(0) else {
            throw BridgeFailure(message: "Task names must not contain control characters.")
        }
        return try taskID.withCString { task in
            try expectedName.withCString { expected in
                try occurredAt.withCString { instant in
                    try decode(
                        tt_bridge_archive_task_with_expected_name_at(handle, task, expected, instant),
                        requiresRefreshOnMalformed: true)
                }
            }
        }
    }

    func unarchiveTask(taskID: String, occurredAt: String) throws -> TaskCommandResult {
        try taskID.withCString { task in
            try occurredAt.withCString { instant in
                try decode(tt_bridge_unarchive_task_at(handle, task, instant), requiresRefreshOnMalformed: true)
            }
        }
    }

    func startTracking(taskID: String, expectedActiveID: String?, occurredAt: String) throws -> TrackingCommandResult {
        try taskID.withCString { task in
            try occurredAt.withCString { instant in
                try withOptionalCString(expectedActiveID) { active in
                    try decode(tt_bridge_start_tracking_if_active_at(handle, task, active, instant))
                }
            }
        }
    }

    func stopTracking(worklogID: String, occurredAt: String) throws -> TrackingCommandResult {
        try worklogID.withCString { worklog in
            try occurredAt.withCString { instant in
                try decode(tt_bridge_stop_tracking_at(handle, worklog, instant))
            }
        }
    }

    func pauseTracking(worklogID: String, occurredAt: String) throws -> TrackingPauseResult {
        try worklogID.withCString { worklog in
            try occurredAt.withCString { instant in
                try decode(tt_bridge_pause_tracking_at(handle, worklog, instant))
            }
        }
    }

    func resumeTracking(taskID: String, occurredAt: String) throws -> TrackingCommandResult {
        try taskID.withCString { task in
            try occurredAt.withCString { instant in
                try decode(tt_bridge_resume_tracking_at(handle, task, instant))
            }
        }
    }

    private func withOptionalCString<Value>(_ value: String?, _ operation: (UnsafePointer<CChar>?) throws -> Value)
        rethrows -> Value
    {
        if let value { return try value.withCString { try operation($0) } }
        return try operation(nil)
    }

    func correctWorklog(
        expected: WorklogItem, replacementStart: String,
        replacementEnd: String?, occurredAt: String
    ) throws -> WorklogCorrectionResult {
        try expected.id.withCString { worklog in
            try expected.start.withCString { originalStart in
                try withOptionalCString(expected.end) { originalEnd in
                    try replacementStart.withCString { start in
                        try withOptionalCString(replacementEnd) { end in
                            try occurredAt.withCString { instant in
                                try decode(
                                    tt_bridge_correct_worklog_at(
                                        handle, worklog, originalStart, originalEnd,
                                        start, end, instant), requiresRefreshOnMalformed: true)
                            }
                        }
                    }
                }
            }
        }
    }

    func history(taskID: String, cursor: String?) throws -> HistoryPage {
        try taskID.withCString { task in
            try withOptionalCString(cursor) { try decode(tt_bridge_history(handle, task, $0)) }
        }
    }

    func moveCandidates(sourceTaskID: String, query: String) throws -> [WorklogMoveCandidate] {
        guard !query.utf8.contains(0) else {
            throw BridgeFailure(message: "Move search queries must not contain null characters.")
        }
        return try sourceTaskID.withCString { source in
            try query.withCString { query in
                try decode(tt_bridge_move_candidates(handle, source, query))
            }
        }
    }

    func moveWorklog(expected: WorklogItem, destinationTaskID: String) throws -> WorklogMoveResult {
        try expected.id.withCString { worklog in
            try expected.taskId.withCString { source in
                try expected.start.withCString { start in
                    try withOptionalCString(expected.end) { end in
                        try destinationTaskID.withCString { destination in
                            try decode(
                                tt_bridge_move_worklog(handle, worklog, source, start, end, destination),
                                requiresRefreshOnMalformed: true)
                        }
                    }
                }
            }
        }
    }

    func deleteWorklog(expected: WorklogItem) throws -> WorklogDeletionResult {
        guard let expectedEnd = expected.end else {
            throw BridgeFailure(message: "Running worklogs cannot be deleted. Stop the timer first.")
        }
        return try expected.id.withCString { worklog in
            try expected.taskId.withCString { task in
                try expected.start.withCString { start in
                    try expectedEnd.withCString { end in
                        try decode(
                            tt_bridge_delete_worklog(handle, worklog, task, start, end),
                            requiresRefreshOnMalformed: true)
                    }
                }
            }
        }
    }
}

// Every handle operation, including creation and destruction, belongs to this queue.
final class TrackerWorker: TrackerClient, ReportClient, @unchecked Sendable {
    private let queue = DispatchQueue(label: "com.timetracker.connection", qos: .utility)
    private let localDatabasePath: String?
    private var bridge: RustBridge?

    init(localDatabasePath: String? = nil) { self.localDatabasePath = localDatabasePath }

    deinit {
        let retainedBridge = bridge
        bridge = nil
        queue.async { retainedBridge?.close() }
    }

    private func perform<Value: Sendable>(_ operation: @escaping @Sendable (TrackerWorker) throws -> Value) async throws
        -> Value
    {
        try await withCheckedThrowingContinuation { continuation in
            queue.async { [self] in
                do { continuation.resume(returning: try operation(self)) } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    private func currentBridge() throws -> RustBridge {
        guard let bridge else { throw BridgeFailure(message: "No tracker connection is open.") }
        return bridge
    }

    private func ensureBridge(_ settings: ConnectionSettings) throws -> RustBridge {
        if let bridge { return bridge }
        let opened = try RustBridge(settings: settings, localDatabasePath: localDatabasePath)
        bridge = opened
        return opened
    }

    func openConfigured(_ settings: ConnectionSettings) async throws -> TaskListResources {
        try await perform { worker in
            // Keep a saved server handle when its first refresh fails. Never fall back to SQLite.
            worker.bridge = try RustBridge(settings: settings, localDatabasePath: worker.localDatabasePath)
            return try worker.currentBridge().refreshTaskList()
        }
    }

    func test(_ settings: ConnectionSettings) async throws {
        try await perform { worker in
            let candidate = try RustBridge(settings: settings, localDatabasePath: worker.localDatabasePath)
            try candidate.testConnection()
        }
    }

    func connect(_ settings: ConnectionSettings) async throws -> TaskListResources {
        try await perform { worker in
            let candidate = try RustBridge(settings: settings, localDatabasePath: worker.localDatabasePath)
            let resources = try candidate.refreshTaskList()
            worker.bridge = candidate
            return resources
        }
    }

    func refreshTaskList(settings: ConnectionSettings) async throws -> TaskListResources {
        try await perform { try $0.ensureBridge(settings).refreshTaskList() }
    }

    func createTask(name: String, occurredAt: String) async throws -> TaskCreationResult {
        try await perform { try $0.currentBridge().createTask(name: name, occurredAt: occurredAt) }
    }

    func renameTask(taskID: String, expectedName: String, name: String, occurredAt: String) async throws
        -> TaskCommandResult
    {
        try await perform {
            try $0.currentBridge().renameTask(
                taskID: taskID, expectedName: expectedName, name: name, occurredAt: occurredAt)
        }
    }

    func previewInactiveTasks(inactiveDays: Int, asOf: String) async throws -> InactiveTaskPreview {
        try await perform { try $0.currentBridge().previewInactiveTasks(inactiveDays: inactiveDays, asOf: asOf) }
    }

    func archiveInactiveTasks(preview: InactiveTaskPreview) async throws -> InactiveTaskArchiveResult {
        try await perform { try $0.currentBridge().archiveInactiveTasks(preview: preview) }
    }

    func archiveTask(taskID: String, expectedName: String, occurredAt: String) async throws -> TaskCommandResult {
        try await perform {
            try $0.currentBridge().archiveTask(taskID: taskID, expectedName: expectedName, occurredAt: occurredAt)
        }
    }

    func unarchiveTask(taskID: String, occurredAt: String) async throws -> TaskCommandResult {
        try await perform { try $0.currentBridge().unarchiveTask(taskID: taskID, occurredAt: occurredAt) }
    }

    func refreshTaskListWithTotals(settings: ConnectionSettings, start: String, end: String, now: String)
        async throws -> TaskListTotalsRefresh
    {
        try await perform {
            try $0.ensureBridge(settings).refreshTaskListWithTotals(start: start, end: end, now: now)
        }
    }

    func startTracking(taskID: String, expectedActiveID: String?, occurredAt: String) async throws
        -> TrackingCommandResult
    {
        try await perform {
            try $0.currentBridge().startTracking(
                taskID: taskID, expectedActiveID: expectedActiveID, occurredAt: occurredAt)
        }
    }

    func stopTracking(worklogID: String, occurredAt: String) async throws -> TrackingCommandResult {
        try await perform { try $0.currentBridge().stopTracking(worklogID: worklogID, occurredAt: occurredAt) }
    }

    func pauseTracking(worklogID: String, occurredAt: String) async throws -> TrackingPauseResult {
        try await perform { try $0.currentBridge().pauseTracking(worklogID: worklogID, occurredAt: occurredAt) }
    }

    func resumeTracking(taskID: String, occurredAt: String) async throws -> TrackingCommandResult {
        try await perform { try $0.currentBridge().resumeTracking(taskID: taskID, occurredAt: occurredAt) }
    }

    func correctWorklog(
        expected: WorklogItem, replacementStart: String,
        replacementEnd: String?, occurredAt: String
    ) async throws -> WorklogCorrectionResult {
        try await perform {
            try $0.currentBridge().correctWorklog(
                expected: expected, replacementStart: replacementStart,
                replacementEnd: replacementEnd, occurredAt: occurredAt)
        }
    }

    func history(taskID: String, cursor: String?) async throws -> HistoryPage {
        try await perform { try $0.currentBridge().history(taskID: taskID, cursor: cursor) }
    }

    func moveCandidates(sourceTaskID: String, query: String) async throws -> [WorklogMoveCandidate] {
        try await perform { try $0.currentBridge().moveCandidates(sourceTaskID: sourceTaskID, query: query) }
    }

    func moveWorklog(expected: WorklogItem, destinationTaskID: String) async throws -> WorklogMoveResult {
        try await perform {
            try $0.currentBridge().moveWorklog(expected: expected, destinationTaskID: destinationTaskID)
        }
    }

    func deleteWorklog(expected: WorklogItem) async throws -> WorklogDeletionResult {
        try await perform { try $0.currentBridge().deleteWorklog(expected: expected) }
    }
}
