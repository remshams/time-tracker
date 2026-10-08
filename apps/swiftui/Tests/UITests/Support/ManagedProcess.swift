import Darwin
import Foundation

final class ManagedProcess {
    let process = Process()
    let log: URL
    private let output: FileHandle
    private let exited = DispatchSemaphore(value: 0)
    private var stopped = false

    init(executable: URL, arguments: [String], log: URL) throws {
        self.log = log
        guard FileManager.default.createFile(atPath: log.path, contents: nil) else {
            throw FixtureError("Could not create child process log.")
        }
        output = try FileHandle(forWritingTo: log)
        process.executableURL = executable
        process.arguments = arguments
        process.standardOutput = output
        process.standardError = output
        process.terminationHandler = { [exited] _ in exited.signal() }
        try process.run()
    }

    func wait(seconds: TimeInterval) throws -> Data {
        guard exited.wait(timeout: .now() + seconds) == .success else {
            stop()
            throw FixtureError("Process timed out: \(process.arguments?.joined(separator: " ") ?? "unknown")")
        }
        stopped = true
        try output.close()
        let data = try Data(contentsOf: log)
        guard process.terminationStatus == 0 else {
            throw FixtureError("Process exited \(process.terminationStatus): \(String(decoding: data, as: UTF8.self))")
        }
        return data
    }

    func stop() {
        guard !stopped else { return }
        stopped = true
        if process.isRunning {
            process.terminate()
            if exited.wait(timeout: .now() + 2) != .success {
                kill(process.processIdentifier, SIGKILL)
                _ = exited.wait(timeout: .now() + 5)
            }
        }
        try? output.close()
    }

    deinit { stop() }
}
