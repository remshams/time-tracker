import Darwin
import Foundation
import SQLite3

struct FixtureTask: Decodable {
    let id: String
    let name: String
    let archived: Bool
}

struct FixtureWorklog: Decodable, Equatable {
    let id: String
    let taskID: String
    let start: String
    let end: String?

    enum CodingKeys: String, CodingKey {
        case id, start, end
        case taskID = "task_id"
    }
}

struct ArchiveSeed {
    let old: FixtureTask
    let medium: FixtureTask
    let recent: FixtureTask
    let running: FixtureTask
    let worklog: FixtureWorklog
}

final class ArchiveFixture {
    let directory: URL
    let database: URL
    let defaultsSuite: String
    private let executable: URL

    init() throws {
        let environment = ProcessInfo.processInfo.environment
        guard let path = environment["TT_UI_TEST_CLI"] ?? environment["TEST_RUNNER_TT_UI_TEST_CLI"], path.hasPrefix("/"),
              FileManager.default.isExecutableFile(atPath: path) else {
            throw FixtureError("Set TT_UI_TEST_CLI to the absolute path of the built tt-cli executable.")
        }
        executable = URL(fileURLWithPath: path)
        let id = UUID().uuidString
        directory = FileManager.default.temporaryDirectory.appendingPathComponent("TimeTrackerUITests-\(id)", isDirectory: true)
        database = directory.appendingPathComponent("tracker.db")
        defaultsSuite = "TimeTrackerUITests.\(id)"
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false,
                                              attributes: [.posixPermissions: 0o700])
    }

    func seed() throws -> ArchiveSeed {
        let now = Date()
        let old = try create("Old planning", at: now.addingTimeInterval(-40 * 86_400))
        let medium = try create("Older review", at: now.addingTimeInterval(-20 * 86_400))
        let recent = try create("Recent release", at: now.addingTimeInterval(-2 * 86_400))
        let running = try create("Running investigation", at: now.addingTimeInterval(-40 * 86_400))
        let started: Started = try command(["tracking", "start", running.id], at: now)
        return ArchiveSeed(old: old, medium: medium, recent: recent, running: running, worklog: started.worklog)
    }

    func tasks() throws -> [FixtureTask] {
        let result: TaskList = try command(["tasks", "list", "--state", "all"])
        return result.tasks.map(\.task)
    }

    func activeWorklog() throws -> FixtureWorklog? {
        let status: Status = try command(["tracking", "status"])
        guard status.state == "running" else { return nil }
        return status.activeWorklog
    }

    func cleanup() throws {
        UserDefaults(suiteName: defaultsSuite)?.removePersistentDomain(forName: defaultsSuite)
        try FileManager.default.removeItem(at: directory)
    }

    func create(_ name: String, at date: Date) throws -> FixtureTask {
        try command(["tasks", "create", name], at: date)
    }

    func withLockedDatabase<Result>(_ operation: () throws -> Result) throws -> Result {
        var connection: OpaquePointer?
        let status = sqlite3_open_v2(database.path, &connection, SQLITE_OPEN_READWRITE, nil)
        guard let connection else { throw FixtureError("Could not open the fixture database for locking.") }
        defer { sqlite3_close(connection) }
        guard status == SQLITE_OK,
              sqlite3_exec(connection, "BEGIN EXCLUSIVE", nil, nil, nil) == SQLITE_OK else {
            throw FixtureError("Could not lock the fixture database: \(String(cString: sqlite3_errmsg(connection)))")
        }
        defer { sqlite3_exec(connection, "ROLLBACK", nil, nil, nil) }
        return try operation()
    }

    private func command<Result: Decodable>(_ arguments: [String], at date: Date = Date()) throws -> Result {
        let output = directory.appendingPathComponent("cli-\(UUID().uuidString).json")
        guard FileManager.default.createFile(atPath: output.path, contents: nil) else {
            throw FixtureError("Could not create CLI output file.")
        }
        let handle = try FileHandle(forWritingTo: output)
        defer {
            try? handle.close()
            try? FileManager.default.removeItem(at: output)
        }
        let process = Process()
        process.executableURL = executable
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        process.arguments = ["--db", database.path, "--at", formatter.string(from: date)] + arguments
        process.standardOutput = handle
        process.standardError = handle
        let exited = DispatchSemaphore(value: 0)
        process.terminationHandler = { _ in exited.signal() }
        try process.run()
        guard exited.wait(timeout: .now() + 20) == .success else {
            process.terminate()
            if exited.wait(timeout: .now() + 2) != .success {
                kill(process.processIdentifier, SIGKILL)
                _ = exited.wait(timeout: .now() + 5)
            }
            throw FixtureError("CLI timed out for \(arguments.joined(separator: " ")).")
        }
        let data = try Data(contentsOf: output)
        guard process.terminationStatus == 0 else {
            throw FixtureError("CLI failed with status \(process.terminationStatus): \(String(decoding: data, as: UTF8.self))")
        }
        let envelope = try JSONDecoder().decode(Envelope<Result>.self, from: data)
        guard envelope.ok else { throw FixtureError("CLI reported an unsuccessful command.") }
        return envelope.data
    }

    private struct Envelope<Result: Decodable>: Decodable {
        let ok: Bool
        let data: Result
    }

    private struct Started: Decodable { let worklog: FixtureWorklog }
    private struct TaskList: Decodable {
        let tasks: [Item]
        struct Item: Decodable { let task: FixtureTask }
    }
    private struct Status: Decodable {
        let state: String
        let activeWorklog: FixtureWorklog?
        enum CodingKeys: String, CodingKey {
            case state
            case activeWorklog = "active_worklog"
        }
    }
}

private struct FixtureError: LocalizedError {
    let errorDescription: String?
    init(_ description: String) { errorDescription = description }
}
