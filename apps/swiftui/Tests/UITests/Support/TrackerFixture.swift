import Darwin
import Foundation
import SQLite3

final class TrackerFixture {
    enum Source { case local, server }
    let directory: URL
    let database: URL
    let defaultsSuite: String
    let source: Source
    private let executable: URL
    private var server: ManagedProcess?
    private var serverEndpoint: String?
    private(set) var proxy: ControlledProxy?
    var serverURL: String? { proxy?.endpoint ?? serverEndpoint }
    var directServerURL: String? { serverEndpoint }

    init(source: Source = .local) throws {
        self.source = source
        executable = try Self.environmentExecutable("TT_UI_TEST_CLI")
        let id = UUID().uuidString
        directory = FileManager.default.temporaryDirectory.appendingPathComponent("TimeTrackerUITests-\(id)", isDirectory: true)
        database = directory.appendingPathComponent("tracker.db")
        defaultsSuite = "TimeTrackerUITests.\(id)"
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false,
                                              attributes: [.posixPermissions: 0o700])
        do {
            if source == .server {
                try startServer()
                try saveConnection(serverURL: serverURL!)
            }
        } catch {
            try? cleanup()
            throw error
        }
    }

    static func environmentExecutable(_ key: String) throws -> URL {
        let env = ProcessInfo.processInfo.environment
        guard let path = env[key] ?? env["TEST_RUNNER_\(key)"], path.hasPrefix("/"),
              FileManager.default.isExecutableFile(atPath: path) else {
            throw FixtureError("Set \(key) to an absolute executable path.")
        }
        return URL(fileURLWithPath: path)
    }

    func saveConnection(serverURL: String?) throws {
        let value = ["mode": serverURL == nil ? "local" : "server", "serverURL": serverURL ?? ""]
        let data = try JSONSerialization.data(withJSONObject: value)
        let defaults = UserDefaults(suiteName: defaultsSuite)!
        defaults.set(data, forKey: "tracker.connection")
        defaults.synchronize()
    }

    func startServer() throws {
        guard server == nil else { return }
        let serverExecutable = try Self.environmentExecutable("TT_UI_TEST_SERVER")
        let existingEndpoint = serverEndpoint
        let attempts = existingEndpoint == nil ? 3 : 1
        for attempt in 1...attempts {
            serverEndpoint = try existingEndpoint ?? "http://127.0.0.1:\(Self.availablePort())"
            let endpoint = URL(string: serverEndpoint!)!
            let child = try ManagedProcess(executable: serverExecutable,
                                           arguments: ["serve", "--bind", "127.0.0.1:\(endpoint.port!)", "--db", database.path],
                                           log: directory.appendingPathComponent("server-\(UUID().uuidString).log"))
            server = child
            do {
                try Self.waitForHealth(serverEndpoint!, child: child)
                return
            } catch {
                stopServer()
                if attempt == attempts { throw error }
            }
        }
    }

    func stopServer() { server?.stop(); server = nil }
    func restartServer() throws { stopServer(); try startServer() }

    @discardableResult
    func enableProxy() throws -> ControlledProxy {
        if let proxy { return proxy }
        guard let serverEndpoint else { throw FixtureError("A proxy requires a server fixture.") }
        let created = try ControlledProxy(directory: directory, upstream: serverEndpoint)
        proxy = created
        try saveConnection(serverURL: created.endpoint)
        return created
    }

    func create(_ name: String, at date: Date = Date()) throws -> FixtureTask {
        try command(["tasks", "create", name], at: date)
    }
    func rename(_ task: FixtureTask, to name: String, at date: Date = Date()) throws -> FixtureTask {
        try command(["tasks", "rename", task.id, name], at: date)
    }
    func archive(_ task: FixtureTask, at date: Date = Date()) throws -> FixtureTask {
        try command(["tasks", "archive", task.id], at: date)
    }
    func restore(_ task: FixtureTask, at date: Date = Date()) throws -> FixtureTask {
        try command(["tasks", "restore", task.id], at: date)
    }
    func start(_ task: FixtureTask, at date: Date = Date()) throws -> FixtureWorklog {
        let result: Started = try command(["tracking", "start", task.id], at: date)
        return result.worklog
    }
    func stop(_ worklog: FixtureWorklog, at date: Date = Date()) throws {
        let _: Empty = try command(["tracking", "stop", "--expected-active", worklog.id], at: date)
    }
    func completed(_ task: FixtureTask, start: Date, end: Date) throws -> FixtureWorklog {
        let running = try self.start(task, at: start)
        try stop(running, at: end)
        let page: HistoryPage = try command(["worklogs", "list", "--task", task.id])
        guard let saved = page.worklogs.first(where: { $0.id == running.id }) else {
            throw FixtureError("Completed worklog was not stored.")
        }
        return saved
    }
    func correct(_ worklog: FixtureWorklog, start: Date, end: Date?) throws -> FixtureWorklog {
        try command(["worklogs", "correct", worklog.id, "--expected-start", worklog.start,
                     "--expected-end", worklog.end ?? "running", "--start", Self.timestamp(start),
                     "--end", end.map(Self.timestamp) ?? "running"])
    }
    func move(_ worklog: FixtureWorklog, to task: FixtureTask) throws -> FixtureWorklog {
        try command(["worklogs", "move", worklog.id, task.id, "--expected-task", worklog.taskID,
                     "--expected-start", worklog.start, "--expected-end", worklog.end ?? "running"])
    }
    func tasks() throws -> [FixtureTask] {
        let result: TaskList = try command(["tasks", "list", "--state", "all"])
        return result.tasks.map(\.task)
    }
    func activeWorklog() throws -> FixtureWorklog? {
        let result: Status = try command(["tracking", "status"])
        return result.state == "running" ? result.activeWorklog : nil
    }
    func worklogs(_ task: FixtureTask? = nil) throws -> [FixtureWorklog] {
        var result: [FixtureWorklog] = []
        var cursor: String?
        var seen = Set<String>()
        repeat {
            var arguments = ["worklogs", "list"]
            if let task { arguments += ["--task", task.id] }
            if let cursor { arguments += ["--cursor", cursor] }
            let page: HistoryPage = try command(arguments)
            result += page.worklogs
            cursor = page.nextCursor.map { String(decoding: $0, as: UTF8.self) }
            if let cursor {
                guard seen.insert(cursor).inserted, seen.count <= 100 else {
                    throw FixtureError("History pagination repeated a cursor or exceeded 100 pages.")
                }
            }
        } while cursor != nil
        return result
    }
    func reports(start: Date, end: Date, at date: Date = Date()) throws -> FixtureReport {
        try command(["reports", "--start", Self.timestamp(start), "--end", Self.timestamp(end)], at: date)
    }

    func command<Result: Decodable>(_ arguments: [String], at date: Date = Date()) throws -> Result {
        let backend = source == .server ? ["--server", serverEndpoint!] : ["--db", database.path]
        let child = try ManagedProcess(executable: executable,
                                       arguments: backend + ["--at", Self.timestamp(date)] + arguments,
                                       log: directory.appendingPathComponent("cli-\(UUID().uuidString).json"))
        let data = try child.wait(seconds: 20)
        let envelope = try JSONDecoder().decode(Envelope<Result>.self, from: data)
        guard envelope.ok else { throw FixtureError("CLI reported an unsuccessful command.") }
        return envelope.data
    }

    func withLockedDatabase<Result>(_ operation: () throws -> Result) throws -> Result {
        var connection: OpaquePointer?
        let status = sqlite3_open_v2(database.path, &connection, SQLITE_OPEN_READWRITE, nil)
        guard let connection else { throw FixtureError("Could not open fixture database.") }
        defer { sqlite3_close(connection) }
        guard status == SQLITE_OK, sqlite3_exec(connection, "BEGIN EXCLUSIVE", nil, nil, nil) == SQLITE_OK else {
            throw FixtureError("Could not lock fixture database: \(String(cString: sqlite3_errmsg(connection)))")
        }
        defer { sqlite3_exec(connection, "ROLLBACK", nil, nil, nil) }
        return try operation()
    }
    func cleanup() throws {
        proxy?.stop(); proxy = nil
        stopServer()
        UserDefaults(suiteName: defaultsSuite)?.removePersistentDomain(forName: defaultsSuite)
        if FileManager.default.fileExists(atPath: directory.path) { try FileManager.default.removeItem(at: directory) }
    }
    deinit { try? cleanup() }

    static func timestamp(_ date: Date) -> String {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter.string(from: date)
    }
    static func availablePort() throws -> UInt16 {
        let descriptor = socket(AF_INET, SOCK_STREAM, 0)
        guard descriptor >= 0 else { throw FixtureError("Could not reserve a loopback port.") }
        defer { close(descriptor) }
        var address = sockaddr_in()
        address.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        address.sin_family = sa_family_t(AF_INET)
        address.sin_addr.s_addr = inet_addr("127.0.0.1")
        let bound = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(descriptor, $0, socklen_t(MemoryLayout<sockaddr_in>.size)) }
        }
        guard bound == 0 else { throw FixtureError("Could not bind a loopback port.") }
        var length = socklen_t(MemoryLayout<sockaddr_in>.size)
        let read = withUnsafeMutablePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { getsockname(descriptor, $0, &length) }
        }
        guard read == 0 else { throw FixtureError("Could not read a loopback port.") }
        return UInt16(bigEndian: address.sin_port)
    }
    static func waitForHealth(_ endpoint: String, child: ManagedProcess) throws {
        let limit = Date().addingTimeInterval(15)
        while Date() < limit {
            guard child.process.isRunning else {
                let log = (try? String(contentsOf: child.log, encoding: .utf8)) ?? "No server log."
                throw FixtureError("Server exited before becoming healthy: \(log)")
            }
            let semaphore = DispatchSemaphore(value: 0)
            let success = LockedFlag()
            var request = URLRequest(url: URL(string: endpoint + "/v1/health")!)
            request.timeoutInterval = 1
            let task = URLSession.shared.dataTask(with: request) { _, response, _ in
                success.value = (response as? HTTPURLResponse)?.statusCode == 200
                semaphore.signal()
            }
            task.resume()
            _ = semaphore.wait(timeout: .now() + 2)
            task.cancel()
            if success.value && child.process.isRunning { return }
            Thread.sleep(forTimeInterval: 0.05)
        }
        throw FixtureError("Server did not become healthy at \(endpoint).")
    }

    private struct Envelope<Result: Decodable>: Decodable { let ok: Bool; let data: Result }
    private struct Empty: Decodable {}
    private struct Started: Decodable { let worklog: FixtureWorklog }
    private struct TaskList: Decodable { let tasks: [Item]; struct Item: Decodable { let task: FixtureTask } }
    private struct Status: Decodable {
        let state: String; let activeWorklog: FixtureWorklog?
        enum CodingKeys: String, CodingKey { case state; case activeWorklog = "active_worklog" }
    }
    private struct HistoryPage: Decodable {
        let worklogs: [FixtureWorklog]
        let nextCursor: Data?
        enum CodingKeys: String, CodingKey { case worklogs; case nextCursor = "next_cursor" }
        init(from decoder: Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            worklogs = try container.decode([FixtureWorklog].self, forKey: .worklogs)
            if try container.decodeNil(forKey: .nextCursor) { nextCursor = nil }
            else {
                let value = try container.decode(JSONValue.self, forKey: .nextCursor)
                nextCursor = try JSONEncoder().encode(value)
            }
        }
    }
}

private final class LockedFlag: @unchecked Sendable {
    private let lock = NSLock()
    private var stored = false
    var value: Bool {
        get { lock.lock(); defer { lock.unlock() }; return stored }
        set { lock.lock(); defer { lock.unlock() }; stored = newValue }
    }
}

private enum JSONValue: Codable {
    case string(String), number(Double), object([String: JSONValue]), array([JSONValue]), bool(Bool), null
    init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null }
        else if let v = try? c.decode(String.self) { self = .string(v) }
        else if let v = try? c.decode(Bool.self) { self = .bool(v) }
        else if let v = try? c.decode(Double.self) { self = .number(v) }
        else if let v = try? c.decode([String: JSONValue].self) { self = .object(v) }
        else { self = .array(try c.decode([JSONValue].self)) }
    }
    func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .string(let v): try c.encode(v)
        case .number(let v): try c.encode(v)
        case .object(let v): try c.encode(v)
        case .array(let v): try c.encode(v)
        case .bool(let v): try c.encode(v)
        case .null: try c.encodeNil()
        }
    }
}
