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
        case taskList
        case tasks
        case trackingResource
        case create(name: String, at: String)
        case inactivePreview(days: Int, at: String)
        case archiveInactive(InactiveTaskPreview)
        case archive(task: String, at: String)
        case unarchive(task: String, at: String)
        case rename(task: String, name: String, at: String)
        case correct(expected: WorklogItem, start: String, end: String?, at: String)
        case candidates(source: String, query: String)
        case move(expected: WorklogItem, destination: String)
        case start(task: String, expected: String?, at: String)
        case stop(worklog: String, at: String)
        case pause(worklog: String, at: String)
        case resume(task: String, at: String)
        case history(task: String, cursor: String?)
        case report(settings: ConnectionSettings, start: String, end: String, now: String)
    }

    enum Reply {
        case taskList(TaskListResources)
        case history(HistoryPage)
        case tested
        case inactivePreview(InactiveTaskPreview)
        case archivedInactive(InactiveTaskArchiveResult)
        case created(TaskCreationResult)
        case corrected(WorklogCorrectionResult)
        case candidates([WorklogMoveCandidate])
        case moved(WorklogMoveResult)
        case paused(TrackingPauseResult)
        case report(TaskListTotalsRefresh)
        case reportResource(TrackerReport)
        case catalog(TaskCatalogObservation)
        case trackingResource(TrackingObservation)
        case dailyTotals(DailyTotalsResources)
        case task(TaskCommandResult)
        case tracking(TrackingCommandResult)
    }

    @MainActor
    struct Request {
        let operation: Operation
        let complete: (Result<Reply, Error>) -> Void
        let publishAfterCommand: (TaskListResources) -> Void

        func succeed(_ resources: TaskListResources) {
            switch operation {
            case .rename(let id, _, _), .archive(let id, _), .unarchive(let id, _):
                var task =
                    resources.catalog.value.first { $0.id == id }
                    ?? TaskItem(id: "", name: "Missing task", archived: false, latestStart: nil)
                let desiredName: String?
                let desiredArchived: Bool?
                switch operation {
                case .rename(_, let name, _):
                    desiredName = TaskNameEditingPolicy.normalized(name); desiredArchived = nil
                case .archive: desiredName = nil; desiredArchived = true
                case .unarchive: desiredName = nil; desiredArchived = false
                default: desiredName = nil; desiredArchived = nil
                }
                let receipt: CommandReceipt?
                if resources.catalog.revision != nil {
                    task = TaskItem(
                        id: id, name: desiredName ?? task.name,
                        archived: desiredArchived ?? task.archived, latestStart: task.latestStart)
                    receipt = CommandReceipt(
                        requestId: "command-request", appliedRevision: "command-applied", replayed: false)
                } else {
                    receipt = nil
                }
                if task.id == id, desiredName.map({ $0 == task.name }) ?? true,
                    desiredArchived.map({ $0 == task.archived }) ?? true
                {
                    publishAfterCommand(resources)
                }
                complete(.success(.task(TaskCommandResult(task: task, receipt: receipt))))
            case .start, .stop, .resume:
                publishAfterCommand(resources)
                complete(.success(.tracking(TrackingCommandResult(active: resources.tracking.value))))
            default: complete(.success(.taskList(resources)))
            }
        }
        func succeed(_ page: HistoryPage) { complete(.success(.history(page))) }
        func succeed(_ report: TaskListTotalsRefresh) { complete(.success(.report(report))) }
        func succeed(_ report: TrackerReport) { complete(.success(.reportResource(report))) }
        func paused(_ snapshot: TaskListResources, didStop: Bool = true) {
            publishAfterCommand(snapshot)
            complete(.success(.paused(TrackingPauseResult(active: snapshot.tracking.value, didStop: didStop))))
        }
        func created(task: TaskItem, receipt: CommandReceipt? = nil) {
            complete(.success(.created(TaskCreationResult(task: task, receipt: receipt))))
        }
        func created(taskID: String, snapshot: TaskListResources) {
            let task =
                snapshot.catalog.value.first { $0.id == taskID }
                ?? TaskItem(id: "", name: "Missing task", archived: false, latestStart: nil)
            if !task.id.isEmpty { publishAfterCommand(snapshot) }
            complete(.success(.created(TaskCreationResult(task: task))))
        }
        func corrected(worklog: WorklogItem, snapshot: TaskListResources) {
            if case .correct(let expected, let start, let end, _) = operation,
                worklog.id == expected.id, worklog.taskId == expected.taskId,
                sameWorklogTimestamp(worklog.start, start), sameWorklogTimestamp(worklog.end, end)
            {
                publishAfterCommand(snapshot)
            }
            complete(.success(.corrected(WorklogCorrectionResult(worklog: worklog))))
        }
        func candidates(_ values: [WorklogMoveCandidate]) { complete(.success(.candidates(values))) }
        func moved(worklog: WorklogItem) {
            complete(.success(.moved(WorklogMoveResult(worklog: worklog))))
        }
        func moved(worklog: WorklogItem, snapshot: TaskListResources) {
            if case .move(let expected, let destination) = operation,
                worklog.id == expected.id, worklog.taskId == destination,
                sameWorklogTimestamp(worklog.start, expected.start), sameWorklogTimestamp(worklog.end, expected.end)
            {
                publishAfterCommand(snapshot)
            }
            complete(.success(.moved(WorklogMoveResult(worklog: worklog))))
        }
        func inactivePreview(_ preview: InactiveTaskPreview) { complete(.success(.inactivePreview(preview))) }
        func archivedInactive(count: Int, snapshot: TaskListResources) {
            if count >= 0 { publishAfterCommand(snapshot) }
            complete(.success(.archivedInactive(InactiveTaskArchiveResult(archivedCount: count))))
        }
        func tested() { complete(.success(.tested)) }
        func fail(_ error: Error) { complete(.failure(error)) }
    }

    private var requests: [Request] = []
    private var commandPublication: TaskListResources?
    private var waiters: [(id: UUID, expectation: XCTestExpectation)] = []
    private var outstanding: [UUID: CheckedContinuation<Reply, Error>] = [:]
    private(set) var operations: [Operation] = []
    private(set) var maximumOutstandingRequests = 0

    func next(timeout: TimeInterval = 2, file: StaticString = #filePath, line: UInt = #line) async throws -> Request {
        if !requests.isEmpty { return requests.removeFirst() }
        let description =
            "Next client request after \(operations.last.map { String(describing: $0) } ?? "startup") at \(file):\(line)"
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
            maximumOutstandingRequests = max(maximumOutstandingRequests, outstanding.count)
            let request = Request(
                operation: operation,
                complete: { [weak self] result in
                    self?.outstanding.removeValue(forKey: id)?.resume(with: result)
                },
                publishAfterCommand: { [weak self] resources in
                    self?.commandPublication = resources
                })
            requests.append(request)
            if !waiters.isEmpty { waiters.removeFirst().expectation.fulfill() }
        }
    }

    private func wrongReply(_ expected: String, operation: Operation) -> TestTimeout {
        let message = "Expected a \(expected) reply for \(operation)."
        XCTFail(message)
        return TestTimeout(description: message)
    }

    private func snapshotReply(_ operation: Operation) async throws -> TaskListResources {
        guard case .taskList(let value) = try await perform(operation) else {
            throw wrongReply("snapshot", operation: operation)
        }
        return value
    }

    func readTaskCatalog() async throws -> TaskCatalogObservation {
        guard case .catalog(let value) = try await perform(.tasks) else {
            throw wrongReply("task resource", operation: .tasks)
        }
        return value
    }

    func readTracking() async throws -> TrackingObservation {
        guard case .trackingResource(let value) = try await perform(.trackingResource) else {
            throw wrongReply("tracking resource", operation: .trackingResource)
        }
        return value
    }

    func refreshDailyTotals(settings: ConnectionSettings, start: String, end: String, now: String)
        async throws -> DailyTotalsResources
    {
        let operation = Operation.report(settings: settings, start: start, end: end, now: now)
        guard case .dailyTotals(let value) = try await perform(operation) else {
            throw wrongReply("daily totals", operation: operation)
        }
        return value
    }

    private func taskReply(_ operation: Operation) async throws -> TaskCommandResult {
        guard case .task(let value) = try await perform(operation) else {
            throw wrongReply("task command", operation: operation)
        }
        return value
    }

    private func trackingReply(_ operation: Operation) async throws -> TrackingCommandResult {
        guard case .tracking(let value) = try await perform(operation) else {
            throw wrongReply("tracking command", operation: operation)
        }
        return value
    }

    func openConfigured(_ settings: ConnectionSettings) async throws -> TaskListResources {
        try await snapshotReply(.open(settings))
    }
    func test(_ settings: ConnectionSettings) async throws { _ = try await perform(.test(settings)) }
    func connect(_ settings: ConnectionSettings) async throws -> TaskListResources {
        try await snapshotReply(.connect(settings))
    }
    func refreshTaskList(settings: ConnectionSettings) async throws -> TaskListResources {
        try await snapshotReply(.refresh(settings))
    }
    func refreshTaskList() async throws -> TaskListResources {
        if let resources = commandPublication {
            commandPublication = nil
            operations.append(.taskList)
            return resources
        }
        return try await snapshotReply(.taskList)
    }
    func startTracking(taskID: String, expectedActiveID: String?, occurredAt: String) async throws
        -> TrackingCommandResult
    {
        try await trackingReply(.start(task: taskID, expected: expectedActiveID, at: occurredAt))
    }
    func stopTracking(worklogID: String, occurredAt: String) async throws -> TrackingCommandResult {
        try await trackingReply(.stop(worklog: worklogID, at: occurredAt))
    }
    func pauseTracking(worklogID: String, occurredAt: String) async throws -> TrackingPauseResult {
        guard case .paused(let value) = try await perform(.pause(worklog: worklogID, at: occurredAt)) else {
            throw wrongReply("pause", operation: .pause(worklog: worklogID, at: occurredAt))
        }
        return value
    }
    func resumeTracking(taskID: String, occurredAt: String) async throws -> TrackingCommandResult {
        try await trackingReply(.resume(task: taskID, at: occurredAt))
    }
    func createTask(name: String, occurredAt: String) async throws -> TaskCreationResult {
        let operation = Operation.create(name: name, at: occurredAt)
        guard case .created(let value) = try await perform(operation) else {
            throw wrongReply("creation", operation: operation)
        }
        return value
    }
    func previewInactiveTasks(inactiveDays: Int, asOf: String) async throws -> InactiveTaskPreview {
        let operation = Operation.inactivePreview(days: inactiveDays, at: asOf)
        guard case .inactivePreview(let value) = try await perform(operation) else {
            throw wrongReply("inactive preview", operation: operation)
        }
        return value
    }
    func archiveInactiveTasks(preview: InactiveTaskPreview) async throws -> InactiveTaskArchiveResult {
        let operation = Operation.archiveInactive(preview)
        guard case .archivedInactive(let value) = try await perform(operation) else {
            throw wrongReply("inactive archive", operation: operation)
        }
        return value
    }
    func archiveTask(taskID: String, occurredAt: String) async throws -> TaskCommandResult {
        try await taskReply(.archive(task: taskID, at: occurredAt))
    }
    func unarchiveTask(taskID: String, occurredAt: String) async throws -> TaskCommandResult {
        try await taskReply(.unarchive(task: taskID, at: occurredAt))
    }
    func renameTask(taskID: String, name: String, occurredAt: String) async throws -> TaskCommandResult {
        try await taskReply(.rename(task: taskID, name: name, at: occurredAt))
    }
    func correctWorklog(
        expected: WorklogItem, replacementStart: String, replacementEnd: String?,
        occurredAt: String
    ) async throws -> WorklogCorrectionResult {
        let operation = Operation.correct(
            expected: expected, start: replacementStart, end: replacementEnd, at: occurredAt)
        guard case .corrected(let value) = try await perform(operation) else {
            throw wrongReply("correction", operation: operation)
        }
        return value
    }
    func moveCandidates(sourceTaskID: String, query: String) async throws -> [WorklogMoveCandidate] {
        let operation = Operation.candidates(source: sourceTaskID, query: query)
        guard case .candidates(let value) = try await perform(operation) else {
            throw wrongReply("candidates", operation: operation)
        }
        return value
    }
    func moveWorklog(expected: WorklogItem, destinationTaskID: String) async throws -> WorklogMoveResult {
        let operation = Operation.move(expected: expected, destination: destinationTaskID)
        guard case .moved(let value) = try await perform(operation) else {
            throw wrongReply("move", operation: operation)
        }
        return value
    }
    func history(taskID: String, cursor: String?) async throws -> HistoryPage {
        guard case .history(let value) = try await perform(.history(task: taskID, cursor: cursor)) else {
            throw wrongReply("history", operation: .history(task: taskID, cursor: cursor))
        }
        return value
    }
    func report(settings: ConnectionSettings, start: String, end: String, now: String) async throws -> TrackerReport {
        let operation = Operation.report(settings: settings, start: start, end: end, now: now)
        guard case .reportResource(let value) = try await perform(operation) else {
            throw wrongReply("report", operation: operation)
        }
        return value
    }
    func refreshTaskListWithTotals(settings: ConnectionSettings, start: String, end: String, now: String)
        async throws -> TaskListTotalsRefresh
    {
        let operation = Operation.report(settings: settings, start: start, end: end, now: now)
        guard case .report(let value) = try await perform(operation) else {
            throw wrongReply("task list totals", operation: operation)
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
    var poll: ScheduledAction? { active.last { !$0.repeating && $0.tolerance > 0 } }
    var display: ScheduledAction? { active.last { $0.repeating } }

    func schedule(
        after delay: TimeInterval, repeating: Bool, tolerance: TimeInterval,
        action: @escaping @MainActor () -> Void
    ) -> any TrackerCancellation {
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

    init(
        saved: ConnectionSettings? = nil, pauseOnScreenLock: Bool = false,
        reports: Bool = false, calendar: Calendar = .autoupdatingCurrent,
        lastTrackedTasks: (any LastTrackedTaskRepository)? = nil
    ) {
        reportsEnabled = reports
        let settings = MemorySettings(saved)
        self.settings = settings
        let preferences = MemoryTrackingPreferences(TrackingPreferences(pauseOnScreenLock: pauseOnScreenLock))
        self.preferences = preferences
        session = TrackerSession(
            client: client, clock: clock, scheduler: scheduler, settings: settings,
            trackingPreferences: preferences, lastTrackedTasks: lastTrackedTasks, reports: reports ? client : nil,
            calendar: calendar)
    }

    func cleanup() {
        session.onChange = nil
        session.shutdown()
        client.cancelOutstanding()
    }

    func start(_ snapshot: TaskListResources = emptySnapshot, rows: [TaskReportTotal] = []) async throws {
        session.start()
        let open = try await client.next()
        XCTAssertEqual(open.operation, .open(settings.saved ?? .local))
        open.succeed(snapshot)
        if reportsEnabled {
            let report = try await client.next()
            guard case .report = report.operation else {
                throw TestTimeout(description: "Expected startup report before history.")
            }
            report.succeed(TaskListTotalsRefresh(snapshot: snapshot, rows: rows))
        }
        if let task = snapshot.catalog.value.first(where: { !$0.archived }) {
            let history = try await client.next()
            XCTAssertEqual(history.operation, .history(task: task.id, cursor: nil))
            guard session.isBusy else {
                XCTFail("Startup history must use the session operation gate.")
                throw TestTimeout(description: "Startup history ran outside the session operation gate.")
            }
            history.succeed(emptyPage)
        }
        try await settled()
    }

    func settled() async throws {
        try await waitUntil("session to finish its current operation") { !self.session.isBusy }
    }

    func taskValue<Value, Failure>(
        _ task: Task<Value, Failure>, timeout: TimeInterval = 2,
        file: StaticString = #filePath, line: UInt = #line
    ) async throws -> Value {
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

    func waitUntil(
        _ description: String, timeout: TimeInterval = 2,
        file: StaticString = #filePath, line: UInt = #line,
        _ condition: @escaping @MainActor () -> Bool
    ) async throws {
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

let emptySnapshot = TaskListResources(tasks: [], active: nil)
let emptyPage = HistoryPage(worklogs: [], nextCursor: nil, reset: false)
let firstTask = TaskItem(id: "task-one", name: "First task", archived: false, latestStart: nil)
let secondTask = TaskItem(id: "task-two", name: "Second task", archived: false, latestStart: nil)
let archivedTask = TaskItem(id: "task-archived", name: "Archived task", archived: true, latestStart: nil)
let activeWorklog = WorklogItem(id: "worklog-active", taskId: "task-one", start: "2024-12-31T23:59:30.000Z", end: nil)
let oldWorklog = WorklogItem(
    id: "worklog-old", taskId: "task-one", start: "2024-12-30T09:00:00.000Z", end: "2024-12-30T10:00:00.000Z")
let serverSettings = ConnectionSettings(mode: .server, serverURL: "https://tracker.example")

@MainActor
final class MemoryTrackingPreferences: TrackingPreferencesRepository {
    var saved: TrackingPreferences
    private(set) var writes: [TrackingPreferences] = []
    init(_ preferences: TrackingPreferences = TrackingPreferences()) { saved = preferences }
    func load() -> TrackingPreferences { saved }
    func save(_ preferences: TrackingPreferences) { saved = preferences; writes.append(preferences) }
}

extension TaskListResources {
    init(
        tasks: [TaskItem], active: WorklogItem?, tasksRevision: String? = nil,
        trackingRevision: String? = nil
    ) {
        self.init(
            catalog: TaskCatalogObservation(value: tasks, revision: tasksRevision),
            tracking: TrackingObservation(value: active, revision: trackingRevision))
    }
}

extension TaskListTotalsRefresh {
    init(snapshot: TaskListResources, rows: [TaskReportTotal], revision: String? = nil, now: String? = nil) {
        self.init(taskList: snapshot, report: TrackerReport(rows: rows, revision: revision, now: now))
    }
}

@MainActor
extension DailyTotalsState {
    func accept(_ refresh: TaskListTotalsRefresh, requested: Request, clock: any TrackerClock) {
        accept(refresh.report, tracking: refresh.taskList.tracking, requested: requested, clock: clock)
    }

    func validate(_ refresh: TaskListTotalsRefresh) throws {
        try validate(refresh.report, tracking: refresh.taskList.tracking)
    }
}
