import Foundation
import XCTest
@testable import TrackerClient

struct TestTimeout: Error, CustomStringConvertible {
    let description: String
}

@MainActor
final class FakeClient: TrackerClient, ReportClient {
    enum Operation: Equatable {
        case open(ConnectionSettings)
        case test(ConnectionSettings)
        case connect(ConnectionSettings)
        case refresh(ConnectionSettings)
        case snapshot
        case start(task: String, expected: String?, at: String)
        case stop(worklog: String, at: String)
        case pause(worklog: String, at: String)
        case resume(task: String, at: String)
        case history(task: String, cursor: String?)
        case report(settings: ConnectionSettings, start: String, end: String, now: String)
    }

    enum Reply {
        case snapshot(TrackerSnapshot)
        case history(HistoryPage)
        case tested
        case paused(TrackingPauseResult)
        case report(TrackerReport)
    }

    @MainActor
    struct Request {
        let operation: Operation
        let complete: (Result<Reply, Error>) -> Void

        func succeed(_ snapshot: TrackerSnapshot) { complete(.success(.snapshot(snapshot))) }
        func succeed(_ page: HistoryPage) { complete(.success(.history(page))) }
        func succeed(_ report: TrackerReport) { complete(.success(.report(report))) }
        func paused(_ snapshot: TrackerSnapshot, didStop: Bool = true) {
            complete(.success(.paused(TrackingPauseResult(snapshot: snapshot, didStop: didStop))))
        }
        func tested() { complete(.success(.tested)) }
        func fail(_ error: Error) { complete(.failure(error)) }
    }

    private var requests: [Request] = []
    private var waiters: [(id: UUID, expectation: XCTestExpectation)] = []
    private var outstanding: [UUID: CheckedContinuation<Reply, Error>] = [:]
    private(set) var operations: [Operation] = []

    func next(timeout: TimeInterval = 2, file: StaticString = #filePath, line: UInt = #line) async throws -> Request {
        if !requests.isEmpty { return requests.removeFirst() }
        let description = "Next client request after \(operations.last.map { String(describing: $0) } ?? "startup") at \(file):\(line)"
        let expectation = XCTestExpectation(description: description)
        let id = UUID()
        waiters.append((id, expectation))
        let result = await XCTWaiter.fulfillment(of: [expectation], timeout: timeout)
        waiters.removeAll { $0.id == id }
        guard result == .completed, !requests.isEmpty else {
            cancelOutstanding()
            throw TestTimeout(description: "Timed out waiting for \(description)")
        }
        return requests.removeFirst()
    }

    func cancelOutstanding() {
        let continuations = outstanding.values
        outstanding.removeAll()
        requests.removeAll()
        let expectations = waiters.map(\.expectation)
        waiters.removeAll()
        for expectation in expectations { expectation.fulfill() }
        for continuation in continuations {
            continuation.resume(throwing: CancellationError())
        }
    }

    private func perform(_ operation: Operation) async throws -> Reply {
        operations.append(operation)
        return try await withCheckedThrowingContinuation { continuation in
            let id = UUID()
            outstanding[id] = continuation
            let request = Request(operation: operation) { [weak self] result in
                self?.outstanding.removeValue(forKey: id)?.resume(with: result)
            }
            requests.append(request)
            if !waiters.isEmpty { waiters.removeFirst().expectation.fulfill() }
        }
    }

    private func wrongReply(_ expected: String, operation: Operation) -> TestTimeout {
        let message = "Expected a \(expected) reply for \(operation)."
        XCTFail(message)
        return TestTimeout(description: message)
    }

    private func snapshotReply(_ operation: Operation) async throws -> TrackerSnapshot {
        guard case .snapshot(let value) = try await perform(operation) else {
            throw wrongReply("snapshot", operation: operation)
        }
        return value
    }

    func openConfigured(_ settings: ConnectionSettings) async throws -> TrackerSnapshot {
        try await snapshotReply(.open(settings))
    }
    func test(_ settings: ConnectionSettings) async throws { _ = try await perform(.test(settings)) }
    func connect(_ settings: ConnectionSettings) async throws -> TrackerSnapshot {
        try await snapshotReply(.connect(settings))
    }
    func refresh(settings: ConnectionSettings) async throws -> TrackerSnapshot {
        try await snapshotReply(.refresh(settings))
    }
    func snapshot() async throws -> TrackerSnapshot { try await snapshotReply(.snapshot) }
    func startTracking(taskID: String, expectedActiveID: String?, occurredAt: String) async throws -> TrackerSnapshot {
        try await snapshotReply(.start(task: taskID, expected: expectedActiveID, at: occurredAt))
    }
    func stopTracking(worklogID: String, occurredAt: String) async throws -> TrackerSnapshot {
        try await snapshotReply(.stop(worklog: worklogID, at: occurredAt))
    }
    func pauseTracking(worklogID: String, occurredAt: String) async throws -> TrackingPauseResult {
        guard case .paused(let value) = try await perform(.pause(worklog: worklogID, at: occurredAt)) else {
            throw wrongReply("pause", operation: .pause(worklog: worklogID, at: occurredAt))
        }
        return value
    }
    func resumeTracking(taskID: String, occurredAt: String) async throws -> TrackerSnapshot {
        try await snapshotReply(.resume(task: taskID, at: occurredAt))
    }
    func history(taskID: String, cursor: String?) async throws -> HistoryPage {
        guard case .history(let value) = try await perform(.history(task: taskID, cursor: cursor)) else {
            throw wrongReply("history", operation: .history(task: taskID, cursor: cursor))
        }
        return value
    }
    func report(settings: ConnectionSettings, start: String, end: String, now: String) async throws -> TrackerReport {
        let operation = Operation.report(settings: settings, start: start, end: end, now: now)
        guard case .report(let value) = try await perform(operation) else {
            throw wrongReply("report", operation: operation)
        }
        return value
    }
}

@MainActor
final class FakeClock: TrackerClock {
    var now = Date(timeIntervalSince1970: 1_735_689_600)
    var uptime: TimeInterval = 100
}

final class ScheduledAction: TrackerCancellation {
    let delay: TimeInterval
    let repeating: Bool
    let tolerance: TimeInterval
    private let action: @MainActor () -> Void
    private(set) var cancelled = false

    init(delay: TimeInterval, repeating: Bool, tolerance: TimeInterval, action: @escaping @MainActor () -> Void) {
        self.delay = delay
        self.repeating = repeating
        self.tolerance = tolerance
        self.action = action
    }
    func cancel() { cancelled = true }
    @MainActor func fire() {
        guard !cancelled else { return }
        if !repeating { cancelled = true }
        action()
    }
    @MainActor func deliverQueuedAction() { action() }
}

@MainActor
final class FakeScheduler: TrackerScheduler {
    private(set) var scheduled: [ScheduledAction] = []
    var active: [ScheduledAction] { scheduled.filter { !$0.cancelled } }
    var poll: ScheduledAction? { active.last { !$0.repeating } }
    var display: ScheduledAction? { active.last { $0.repeating } }

    func schedule(after delay: TimeInterval, repeating: Bool, tolerance: TimeInterval,
                  action: @escaping @MainActor () -> Void) -> any TrackerCancellation {
        let token = ScheduledAction(delay: delay, repeating: repeating, tolerance: tolerance, action: action)
        scheduled.append(token)
        return token
    }
}

@MainActor
final class MemorySettings: ConnectionSettingsRepository {
    var saved: ConnectionSettings?
    private(set) var writes: [ConnectionSettings] = []
    init(_ saved: ConnectionSettings? = nil) { self.saved = saved }
    func load() -> ConnectionSettings? { saved }
    func save(_ settings: ConnectionSettings) { saved = settings; writes.append(settings) }
}

@MainActor
final class Fixture {
    let client = FakeClient()
    let clock = FakeClock()
    let scheduler = FakeScheduler()
    let settings: MemorySettings
    let session: TrackerSession
    let preferences: MemoryTrackingPreferences
    private let reportsEnabled: Bool

    init(saved: ConnectionSettings? = nil, pauseOnScreenLock: Bool = false,
         reports: Bool = false, calendar: Calendar = .autoupdatingCurrent) {
        reportsEnabled = reports
        let settings = MemorySettings(saved)
        self.settings = settings
        let preferences = MemoryTrackingPreferences(TrackingPreferences(pauseOnScreenLock: pauseOnScreenLock))
        self.preferences = preferences
        session = TrackerSession(client: client, clock: clock, scheduler: scheduler, settings: settings,
                                 trackingPreferences: preferences, reports: reports ? client : nil, calendar: calendar)
    }

    func cleanup() {
        session.onChange = nil
        session.shutdown()
        client.cancelOutstanding()
    }

    func start(_ snapshot: TrackerSnapshot = emptySnapshot, rows: [TaskReportTotal] = []) async throws {
        session.start()
        let open = try await client.next()
        XCTAssertEqual(open.operation, .open(settings.saved ?? .local))
        open.succeed(snapshot)
        if reportsEnabled {
            let report = try await client.next()
            guard case .report = report.operation else { throw TestTimeout(description: "Expected startup report before history.") }
            report.succeed(TrackerReport(snapshot: snapshot, rows: rows))
        }
        if let task = snapshot.tasks.first(where: { !$0.archived }) {
            let history = try await client.next()
            XCTAssertEqual(history.operation, .history(task: task.id, cursor: nil))
            guard session.isBusy else {
                XCTFail("Fetching startup history must keep conflicting commands disabled.")
                throw TestTimeout(description: "Startup history ran outside the session operation gate.")
            }
            history.succeed(emptyPage)
        }
        try await settled()
    }

    func settled() async throws { try await waitUntil("session to finish its current operation") { !self.session.isBusy } }

    func taskValue<Value, Failure>(_ task: Task<Value, Failure>, timeout: TimeInterval = 2,
                                   file: StaticString = #filePath, line: UInt = #line) async throws -> Value {
        let expectation = XCTestExpectation(description: "Asynchronous test task at \(file):\(line)")
        var completedResult: Result<Value, Failure>?
        let observer = Task { @MainActor in
            completedResult = await task.result
            expectation.fulfill()
        }
        let result = await XCTWaiter.fulfillment(of: [expectation], timeout: timeout)
        guard result == .completed, let completedResult else {
            cleanup()
            task.cancel()
            observer.cancel()
            throw TestTimeout(description: "Timed out waiting for asynchronous test task at \(file):\(line)")
        }
        return try completedResult.get()
    }

    func waitUntil(_ description: String, timeout: TimeInterval = 2,
                   file: StaticString = #filePath, line: UInt = #line,
                   _ condition: @escaping @MainActor () -> Bool) async throws {
        if condition() { return }
        let expectation = XCTestExpectation(description: description)
        session.onChange = { [weak session] in
            if condition() {
                session?.onChange = nil
                expectation.fulfill()
            }
        }
        let result = await XCTWaiter.fulfillment(of: [expectation], timeout: timeout)
        session.onChange = nil
        guard result == .completed else {
            cleanup()
            throw TestTimeout(description: "Timed out waiting for \(description) at \(file):\(line)")
        }
    }
}

let emptySnapshot = TrackerSnapshot(tasks: [], active: nil)
let emptyPage = HistoryPage(worklogs: [], nextCursor: nil, reset: false)
let firstTask = TaskItem(id: "task-one", name: "First task", archived: false, latestStart: nil)
let secondTask = TaskItem(id: "task-two", name: "Second task", archived: false, latestStart: nil)
let archivedTask = TaskItem(id: "task-archived", name: "Archived task", archived: true, latestStart: nil)
let activeWorklog = WorklogItem(id: "worklog-active", taskId: "task-one", start: "2024-12-31T23:59:30.000Z", end: nil)
let oldWorklog = WorklogItem(id: "worklog-old", taskId: "task-one", start: "2024-12-30T09:00:00.000Z", end: "2024-12-30T10:00:00.000Z")
let serverSettings = ConnectionSettings(mode: .server, serverURL: "https://tracker.example")

@MainActor
final class MemoryTrackingPreferences: TrackingPreferencesRepository {
    var saved: TrackingPreferences
    private(set) var writes: [TrackingPreferences] = []
    init(_ preferences: TrackingPreferences = TrackingPreferences()) { saved = preferences }
    func load() -> TrackingPreferences { saved }
    func save(_ preferences: TrackingPreferences) { saved = preferences; writes.append(preferences) }
}
