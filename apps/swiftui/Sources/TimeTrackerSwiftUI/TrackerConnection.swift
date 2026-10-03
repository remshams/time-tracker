import Foundation

enum ConnectionMode: String, CaseIterable, Codable, Identifiable, Sendable {
    case local
    case server

    var id: Self { self }
    var label: String { self == .local ? "Local database" : "Server" }
}

struct ConnectionSettings: Codable, Equatable, Sendable {
    var mode: ConnectionMode
    var serverURL: String

    static let local = ConnectionSettings(mode: .local, serverURL: "")
}

struct BridgeFailure: LocalizedError, Sendable {
    let message: String
    var kind = "general"
    var uncertain = false
    var requiresRefresh = false
    var errorDescription: String? { message }
}

private struct BridgeEnvelope<Value: Decodable>: Decodable {
    let data: Value?
    let error: String?
    let kind: String?
    let uncertain: Bool?
    let requiresRefresh: Bool?
}

private final class RustBridge {
    private let handle: OpaquePointer

    init(settings: ConnectionSettings) throws {
        var error: UnsafeMutablePointer<CChar>?
        let opened: OpaquePointer?
        if settings.mode == .server {
            opened = settings.serverURL.withCString { tt_bridge_open_remote($0, &error) }
        } else {
            opened = tt_bridge_open(&error)
        }
        defer { if let error { tt_bridge_string_free(error) } }
        guard let opened else {
            throw BridgeFailure(message: error.map { String(cString: $0) } ?? "Could not open the connection.")
        }
        handle = opened
    }

    deinit { tt_bridge_close(handle) }

    private func decode<Value: Decodable>(_ pointer: UnsafeMutablePointer<CChar>?) throws -> Value {
        guard let pointer else { throw BridgeFailure(message: "The tracker bridge returned no data.") }
        defer { tt_bridge_string_free(pointer) }
        let envelope = try JSONDecoder().decode(BridgeEnvelope<Value>.self, from: Data(String(cString: pointer).utf8))
        if let error = envelope.error {
            throw BridgeFailure(message: error, kind: envelope.kind ?? "general",
                                uncertain: envelope.uncertain ?? false,
                                requiresRefresh: envelope.requiresRefresh ?? false)
        }
        guard let data = envelope.data else { throw BridgeFailure(message: "The tracker bridge returned an empty result.") }
        return data
    }

    func snapshot() throws -> TrackerSnapshot { try decode(tt_bridge_snapshot(handle, true)) }

    func startTracking(taskID: String, expectedActiveID: String?, occurredAt: String) throws -> TrackerSnapshot {
        try taskID.withCString { task in
            try occurredAt.withCString { instant in
                if let expectedActiveID {
                    return try expectedActiveID.withCString {
                        try decode(tt_bridge_start_tracking_if_active_at(handle, task, $0, instant))
                    }
                }
                return try decode(tt_bridge_start_tracking_if_active_at(handle, task, nil, instant))
            }
        }
    }

    func stopTracking(worklogID: String, occurredAt: String) throws -> TrackerSnapshot {
        try worklogID.withCString { worklog in
            try occurredAt.withCString { instant in
                try decode(tt_bridge_stop_tracking_at(handle, worklog, instant))
            }
        }
    }

    func history(taskID: String, cursor: String?) throws -> HistoryPage {
        try taskID.withCString { task in
            if let cursor {
                return try cursor.withCString { try decode(tt_bridge_history(handle, task, $0)) }
            }
            return try decode(tt_bridge_history(handle, task, nil))
        }
    }
}

// Every handle operation, including creation and destruction, belongs to this queue.
final class TrackerWorker: @unchecked Sendable {
    private let queue = DispatchQueue(label: "com.timetracker.connection", qos: .utility)
    private var bridge: RustBridge?

    deinit {
        let retainedBridge = bridge
        queue.async { _ = retainedBridge }
    }

    private func perform<Value: Sendable>(_ operation: @escaping @Sendable (TrackerWorker) throws -> Value) async throws -> Value {
        try await withCheckedThrowingContinuation { continuation in
            queue.async { [self] in
                do { continuation.resume(returning: try operation(self)) }
                catch { continuation.resume(throwing: error) }
            }
        }
    }

    private func currentBridge() throws -> RustBridge {
        guard let bridge else { throw BridgeFailure(message: "No tracker connection is open.") }
        return bridge
    }

    func openConfigured(_ settings: ConnectionSettings) async throws -> TrackerSnapshot {
        try await perform { worker in
            // Keep a saved server handle when its first refresh fails. Never fall back to SQLite.
            worker.bridge = try RustBridge(settings: settings)
            return try worker.currentBridge().snapshot()
        }
    }

    func test(_ settings: ConnectionSettings) async throws {
        let _: TrackerSnapshot = try await perform { _ in
            let candidate = try RustBridge(settings: settings)
            return try candidate.snapshot()
        }
    }

    func connect(_ settings: ConnectionSettings) async throws -> TrackerSnapshot {
        try await perform { worker in
            let candidate = try RustBridge(settings: settings)
            let snapshot = try candidate.snapshot()
            worker.bridge = candidate
            return snapshot
        }
    }

    func refresh(settings: ConnectionSettings) async throws -> TrackerSnapshot {
        try await perform { worker in
            if worker.bridge == nil { worker.bridge = try RustBridge(settings: settings) }
            return try worker.currentBridge().snapshot()
        }
    }

    func snapshot() async throws -> TrackerSnapshot {
        try await perform { try $0.currentBridge().snapshot() }
    }

    func startTracking(taskID: String, expectedActiveID: String?, occurredAt: String) async throws -> TrackerSnapshot {
        try await perform { try $0.currentBridge().startTracking(taskID: taskID, expectedActiveID: expectedActiveID, occurredAt: occurredAt) }
    }

    func stopTracking(worklogID: String, occurredAt: String) async throws -> TrackerSnapshot {
        try await perform { try $0.currentBridge().stopTracking(worklogID: worklogID, occurredAt: occurredAt) }
    }

    func history(taskID: String, cursor: String?) async throws -> HistoryPage {
        try await perform { try $0.currentBridge().history(taskID: taskID, cursor: cursor) }
    }
}

struct TrackerPollingPolicy {
    static func interval(visible: Bool, failures: Int) -> TimeInterval {
        let base: TimeInterval = visible ? 5 : 60
        let backoff = 5 * pow(2, Double(min(max(failures - 1, 0), 4)))
        return max(base, min(60, backoff))
    }
}
