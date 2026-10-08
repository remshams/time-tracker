import Foundation

final class ControlledProxy {
    enum Mode: String { case holdBefore, holdAfter, dropAfter, fail, invalidProtocol }
    struct Rule {
        let method: String
        let path: String
        let mode: Mode
    }
    struct Request: Decodable {
        let id: Int
        let method: String
        let path: String
        let body: String
        let status: Int?
    }
    let endpoint: String
    private let directory: URL
    private let process: ManagedProcess
    private var rules: [[String: String]] = []
    private var heldToken: String?
    private var released: [String] = []

    init(directory: URL, upstream: String) throws {
        self.directory = directory
        let environment = ProcessInfo.processInfo.environment
        guard let script = environment["TT_UI_TEST_PROXY"] ?? environment["TEST_RUNNER_TT_UI_TEST_PROXY"],
              script.hasPrefix("/"), FileManager.default.fileExists(atPath: script) else {
            throw FixtureError("Set TT_UI_TEST_PROXY to the absolute controlled_proxy.py path.")
        }
        let python = environment["TT_UI_TEST_PYTHON"] ?? environment["TEST_RUNNER_TT_UI_TEST_PYTHON"] ?? "/usr/bin/python3"
        process = try ManagedProcess(executable: URL(fileURLWithPath: python),
                                     arguments: [script, "--directory", directory.path, "--upstream", upstream],
                                     log: directory.appendingPathComponent("proxy.log"))
        let limit = Date().addingTimeInterval(10)
        let output = directory.appendingPathComponent("proxy-endpoint.json")
        var discovered: String?
        while Date() < limit {
            guard process.process.isRunning else { break }
            if let data = try? Data(contentsOf: output),
               let value = try? JSONDecoder().decode(Endpoint.self, from: data) {
                discovered = value.endpoint
                break
            }
            Thread.sleep(forTimeInterval: 0.02)
        }
        guard let discovered else {
            process.stop()
            let log = (try? String(contentsOf: process.log, encoding: .utf8)) ?? "No proxy log."
            let status = process.process.isRunning ? "still running" : String(process.process.terminationStatus)
            throw FixtureError("Proxy did not start using \(python), status \(status): \(log)")
        }
        endpoint = discovered
    }

    func arm(method: String, path: String, mode: Mode) throws {
        try arm([Rule(method: method, path: path, mode: mode)])
    }
    func arm(_ plan: [Rule]) throws {
        try reset()
        rules = plan.map { ["token": UUID().uuidString, "method": $0.method, "path": $0.path, "mode": $0.mode.rawValue] }
        heldToken = nil
        try write(["rules": rules, "released": released])
    }
    @discardableResult
    func waitForHeldRequest(timeout: TimeInterval = 15) throws -> Request {
        let limit = Date().addingTimeInterval(timeout)
        while Date() < limit {
            if let data = try? Data(contentsOf: directory.appendingPathComponent("proxy-held.json")),
               let held = try? JSONDecoder().decode(Held.self, from: data),
               rules.contains(where: { $0["token"] == held.token }) {
                heldToken = held.token
                return held.request
            }
            Thread.sleep(forTimeInterval: 0.02)
        }
        throw FixtureError("No request reached the proxy gate.")
    }
    func release() throws {
        if let heldToken { released.append(heldToken) }
        else { released += rules.compactMap { $0["token"] } }
        try write(["rules": rules, "released": released])
    }
    func reset() throws {
        released += rules.compactMap { $0["token"] }.filter { !released.contains($0) }
        try write(["rules": [], "released": released])
        rules = []
        heldToken = nil
    }
    func requests(method: String? = nil, path: String? = nil) throws -> [Request] {
        let output = directory.appendingPathComponent("proxy-requests.json")
        guard FileManager.default.fileExists(atPath: output.path) else { return [] }
        let all = try JSONDecoder().decode([Request].self, from: Data(contentsOf: output))
        return all.filter { (method == nil || $0.method == method) && (path == nil || $0.path.contains(path!)) }
    }
    func stop() { try? reset(); process.stop() }
    private func write(_ value: [String: Any]) throws {
        try JSONSerialization.data(withJSONObject: value).write(
            to: directory.appendingPathComponent("proxy-control.json"), options: .atomic)
    }
    private struct Endpoint: Decodable { let endpoint: String }
    private struct Held: Decodable { let token: String; let request: Request }
}
