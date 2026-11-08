import XCTest
@testable import TrackerClient

final class HistoryTests: XCTestCase {
    @MainActor
    func testSelectingAnotherTaskShowsLoadingWithoutPreviousRowsOrCursor() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        fixture.sync.retryHistory()
        let initial = try await fixture.client.next()
        initial.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "older", reset: false))
        try await fixture.settled()

        fixture.sync.selectTask(secondTask.id)
        XCTAssertTrue(fixture.session.worklogHistory.isLoading)
        XCTAssertTrue(fixture.session.worklogHistory.worklogs.isEmpty)
        XCTAssertNil(fixture.session.worklogHistory.nextCursor)
        let selected = try await fixture.client.next()
        XCTAssertEqual(selected.operation, .history(task: secondTask.id, cursor: nil))
        selected.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.worklogHistory.isLoading)
    }

    @MainActor
    func testPollingKeepsExistingRowsWhileSameTaskHistoryRefreshes() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        let running = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(running)
        fixture.sync.retryHistory()
        let initial = try await fixture.client.next()
        initial.succeed(HistoryPage(worklogs: [activeWorklog, oldWorklog], nextCursor: "older", reset: false))
        try await fixture.settled()

        fixture.scheduler.poll?.fire()
        let refresh = try await fixture.client.next()
        refresh.succeed(TaskListResources(tasks: [firstTask], active: nil))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        XCTAssertEqual(
            fixture.session.worklogHistory.worklogs, [activeWorklog, oldWorklog],
            "Existing rows must remain visible while their replacement is fetched.")
        XCTAssertEqual(fixture.session.worklogHistory.nextCursor, "older")
        XCTAssertTrue(fixture.session.worklogHistory.isLoading)
        XCTAssertTrue(fixture.session.worklogHistory.hasMore)
        fixture.sync.loadOlder()
        history.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [oldWorklog])
        XCTAssertNil(fixture.session.worklogHistory.nextCursor)
    }

    @MainActor
    func testRefreshingHistoryRetainsPaginationButRejectsLoadOlderUntilConfirmed() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.sync.retryHistory()
        let initial = try await fixture.client.next()
        let page = HistoryPage(worklogs: [oldWorklog], nextCursor: "older", reset: false)
        initial.succeed(page)
        try await fixture.settled()
        fixture.sync.retryHistory()
        let refresh = try await fixture.client.next()
        XCTAssertEqual(fixture.session.worklogHistory.nextCursor, "older")
        XCTAssertTrue(fixture.session.worklogHistory.hasMore)
        XCTAssertTrue(fixture.session.worklogHistory.isLoading)
        fixture.sync.loadOlder()
        refresh.succeed(page)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.worklogHistory.isLoading)
        XCTAssertTrue(fixture.session.worklogHistory.hasMore)
        fixture.sync.loadOlder()
        let older = try await fixture.client.next()
        XCTAssertEqual(older.operation, .history(task: firstTask.id, cursor: "older"))
        older.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.worklogHistory.hasMore)
    }

    @MainActor
    func testIncompatibleHistoryDisablesTrackingAndStopsAutomaticPolling() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        XCTAssertTrue(fixture.canStartSelectedTask)
        XCTAssertNotNil(fixture.scheduler.poll)
        fixture.sync.retryHistory()
        let history = try await fixture.client.next()
        history.fail(BridgeFailure(message: "Unsupported history version", kind: "protocol"))
        try await fixture.settled()

        XCTAssertTrue(fixture.session.worklogHistory.unavailable)
        XCTAssertTrue(fixture.session.connection.stale)
        XCTAssertFalse(fixture.canStartSelectedTask)
        XCTAssertEqual(fixture.session.connection.statusText, "Incompatible server")
        XCTAssertEqual(fixture.session.connection.message, "Unsupported history version")
        XCTAssertNil(fixture.scheduler.poll)
    }

    @MainActor
    func testHistoryRequiringRefreshDisablesTrackingUntilResourcesAreConfirmed() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources)
        XCTAssertTrue(fixture.canStartSelectedTask)
        fixture.sync.retryHistory()
        let history = try await fixture.client.next()
        history.fail(BridgeFailure(message: "Tracker state must be refreshed", requiresRefresh: true))
        try await fixture.settled()

        XCTAssertTrue(fixture.session.worklogHistory.unavailable)
        XCTAssertTrue(fixture.session.connection.stale)
        XCTAssertFalse(fixture.canStartSelectedTask)
        XCTAssertEqual(fixture.session.connection.statusText, "Unavailable")
        XCTAssertEqual(fixture.session.connection.message, "Tracker state must be refreshed")

        fixture.sync.refresh()
        let refresh = try await fixture.client.next()
        XCTAssertFalse(fixture.canStartSelectedTask)
        refresh.succeed(resources)
        let retry = try await fixture.client.next()
        retry.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.connection.stale)
        XCTAssertFalse(fixture.session.worklogHistory.unavailable)
        XCTAssertTrue(fixture.canStartSelectedTask)
    }

    @MainActor
    func testSelectionsAreRememberedPerTabAndRemovedSelectionFallsBack() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: nil)
        try await fixture.start(resources)
        fixture.sync.selectTask(secondTask.id)
        let selected = try await fixture.client.next()
        selected.succeed(emptyPage)
        try await fixture.settled()
        fixture.sync.changeTab(.archived)
        let archived = try await fixture.client.next()
        XCTAssertEqual(archived.operation, .history(task: archivedTask.id, cursor: nil))
        archived.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.taskCatalog.visibleTasks, [archivedTask])
        fixture.sync.selectTask(firstTask.id)
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, archivedTask.id)
        fixture.sync.changeTab(.active)
        let restored = try await fixture.client.next()
        XCTAssertEqual(restored.operation, .history(task: secondTask.id, cursor: nil))
        restored.succeed(emptyPage)
        try await fixture.settled()
        fixture.sync.refresh()
        let refresh = try await fixture.client.next()
        refresh.succeed(TaskListResources(tasks: [firstTask, archivedTask], active: nil))
        let fallback = try await fixture.client.next()
        XCTAssertEqual(fallback.operation, .history(task: firstTask.id, cursor: nil))
        fallback.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, firstTask.id)
    }

    @MainActor
    func testDelayedHistoryCannotReplaceNewSelectionOrItsErrorState() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        fixture.sync.retryHistory()
        let oldRequest = try await fixture.client.next()
        fixture.sync.selectTask(secondTask.id)
        XCTAssertTrue(fixture.session.worklogHistory.isLoading)
        XCTAssertTrue(fixture.session.worklogHistory.worklogs.isEmpty)
        oldRequest.fail(BridgeFailure(message: "Old selection failed", kind: "unavailable"))
        let newRequest = try await fixture.client.next()
        XCTAssertEqual(newRequest.operation, .history(task: secondTask.id, cursor: nil))
        XCTAssertFalse(fixture.session.worklogHistory.unavailable)
        XCTAssertTrue(fixture.session.worklogHistory.isLoading)
        XCTAssertNil(fixture.session.worklogHistory.error)
        XCTAssertFalse(fixture.session.connection.stale)
        newRequest.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, secondTask.id)
        XCTAssertFalse(fixture.session.worklogHistory.unavailable)
        XCTAssertNil(fixture.session.worklogHistory.error)
    }

    @MainActor
    func testDelayedSuccessfulHistoryCannotPopulateNewSelection() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        fixture.sync.retryHistory()
        let oldRequest = try await fixture.client.next()
        fixture.sync.selectTask(secondTask.id)
        oldRequest.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "obsolete", reset: false))
        let newRequest = try await fixture.client.next()
        XCTAssertTrue(fixture.session.worklogHistory.worklogs.isEmpty)
        XCTAssertNil(fixture.session.worklogHistory.nextCursor)
        newRequest.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertTrue(fixture.session.worklogHistory.worklogs.isEmpty)
    }

    @MainActor
    func testReturningToTaskAcceptsItsEarlierHistoryRequest() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        fixture.sync.retryHistory()
        let earlier = try await fixture.client.next()
        fixture.sync.selectTask(secondTask.id)
        let other = try await fixture.client.next()
        fixture.sync.selectTask(firstTask.id)
        let current = try await fixture.client.next()
        try await fixture.update("the current page") { current.succeed(emptyPage) }
        try await fixture.update("the earlier page for the selected task") {
            earlier.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "older", reset: false))
        }
        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [oldWorklog])
        XCTAssertEqual(fixture.session.worklogHistory.nextCursor, "older")
        other.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [oldWorklog])
        XCTAssertFalse(fixture.session.connection.stale)
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 3)
    }

    @MainActor
    func testPaginationAppendsButResetReplacesPreviousRowsAndCursor() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.sync.retryHistory()
        let first = try await fixture.client.next()
        first.succeed(HistoryPage(worklogs: [activeWorklog], nextCursor: "page-two", reset: false))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.worklogHistory.hasMore)
        fixture.sync.loadOlder()
        let older = try await fixture.client.next()
        XCTAssertEqual(older.operation, .history(task: firstTask.id, cursor: "page-two"))
        XCTAssertTrue(fixture.session.worklogHistory.isLoading)
        XCTAssertTrue(fixture.session.worklogHistory.hasMore)
        fixture.sync.loadOlder()
        older.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "page-three", reset: false))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [activeWorklog, oldWorklog])
        fixture.sync.loadOlder()
        let reset = try await fixture.client.next()
        XCTAssertEqual(reset.operation, .history(task: firstTask.id, cursor: "page-three"))
        reset.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: nil, reset: true))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [oldWorklog])
        XCTAssertFalse(fixture.session.worklogHistory.hasMore)
    }

    @MainActor
    func testHistoryFailureRetriesCurrentSelectionAndClearsError() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.sync.retryHistory()
        let failure = try await fixture.client.next()
        failure.fail(BridgeFailure(message: "History read failed"))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.worklogHistory.unavailable)
        XCTAssertFalse(fixture.session.worklogHistory.isLoading)
        XCTAssertEqual(fixture.session.worklogHistory.error, "History read failed")
        XCTAssertFalse(fixture.session.connection.stale)
        fixture.sync.retryHistory()
        let retry = try await fixture.client.next()
        XCTAssertTrue(fixture.session.worklogHistory.isLoading)
        XCTAssertEqual(retry.operation, .history(task: firstTask.id, cursor: nil))
        retry.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [oldWorklog])
        XCTAssertFalse(fixture.session.worklogHistory.unavailable)
        XCTAssertNil(fixture.session.worklogHistory.error)
    }

    @MainActor
    func testLoadingAnOlderPageAfterAFailedReloadClearsTheError() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.sync.retryHistory()
        try await fixture.client.next().succeed(
            HistoryPage(worklogs: [activeWorklog], nextCursor: "older", reset: false))
        try await fixture.settled()
        fixture.sync.retryHistory()
        try await fixture.client.next().fail(BridgeFailure(message: "History read failed"))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.worklogHistory.unavailable)

        fixture.sync.loadOlder()
        let older = try await fixture.client.next()
        XCTAssertEqual(older.operation, .history(task: firstTask.id, cursor: "older"))
        older.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [activeWorklog, oldWorklog])
        XCTAssertFalse(fixture.session.worklogHistory.unavailable)
        XCTAssertNil(fixture.session.worklogHistory.error)
    }

    @MainActor
    func testUnavailableHistoryMarksResourcesStaleAndRefreshRetriesHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources)
        fixture.sync.retryHistory()
        let history = try await fixture.client.next()
        history.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.connection.stale)
        XCTAssertFalse(fixture.canStartSelectedTask)
        fixture.sync.refresh()
        let refresh = try await fixture.client.next()
        refresh.succeed(resources)
        let retry = try await fixture.client.next()
        XCTAssertTrue(fixture.session.worklogHistory.isLoading)
        XCTAssertEqual(retry.operation, .history(task: firstTask.id, cursor: nil))
        retry.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.connection.stale)
        XCTAssertFalse(fixture.session.worklogHistory.unavailable)
        XCTAssertTrue(fixture.canStartSelectedTask)
    }

    @MainActor
    func testPageForAnotherTaskNeverReachesTheHistoryReaders() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        fixture.sync.retryHistory()
        let oldRequest = try await fixture.client.next()
        fixture.sync.selectTask(secondTask.id)
        let probes = HistoryProbes(fixture.session.worklogHistory)

        oldRequest.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "obsolete", reset: false))
        let newRequest = try await fixture.client.next()
        newRequest.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(probes.changed(), ["isLoading"])
        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [])
    }

    @MainActor
    func testUnchangedPollNotifiesNoHistoryReader() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources)
        let probes = HistoryProbes(fixture.session.worklogHistory)

        fixture.scheduler.poll?.fire()
        try await fixture.client.next().succeed(resources)
        try await fixture.settled()
        XCTAssertEqual(probes.changed(), [])
    }

    @MainActor
    func testLoadingOlderPagesNotifiesHasMoreReadersOnlyWhenItFlips() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.sync.retryHistory()
        try await fixture.client.next().succeed(
            HistoryPage(worklogs: [activeWorklog], nextCursor: "page-two", reset: false))
        try await fixture.settled()
        let probes = HistoryProbes(fixture.session.worklogHistory)

        fixture.sync.loadOlder()
        XCTAssertEqual(probes.changed(), ["isLoading"])
        try await fixture.client.next().succeed(
            HistoryPage(worklogs: [oldWorklog], nextCursor: "page-three", reset: false))
        try await fixture.settled()
        XCTAssertEqual(probes.changed(), ["worklogs", "isLoading"])
        fixture.sync.loadOlder()
        XCTAssertEqual(probes.changed(), ["isLoading"])
        try await fixture.client.next().succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(probes.changed(), ["hasMore", "isLoading"])
        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [activeWorklog, oldWorklog])
    }

    @MainActor
    func testConnectionFailureAndRecoveryNotifyOnlyErrorReaders() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources)
        let probes = HistoryProbes(fixture.session.worklogHistory)

        fixture.scheduler.poll?.fire()
        try await fixture.client.next().fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertEqual(probes.changed(), ["error"])
        XCTAssertEqual(fixture.session.worklogHistory.error, "Server offline")
        XCTAssertTrue(fixture.session.connection.stale)
        fixture.scheduler.poll?.fire()
        try await fixture.client.next().succeed(resources)
        try await fixture.settled()
        XCTAssertEqual(probes.changed(), ["error"])
        XCTAssertNil(fixture.session.worklogHistory.error)
        XCTAssertFalse(fixture.session.connection.stale)
    }
}

/// Reports which history properties changed, in declaration order, as views reading each one would see them.
@MainActor
private final class HistoryProbes {
    private let probes: [(String, ObservationProbe)]

    init(_ store: WorklogHistoryStore) {
        probes = [
            ("worklogs", ObservationProbe { _ = store.worklogs }),
            ("hasMore", ObservationProbe { _ = store.hasMore }),
            ("unavailable", ObservationProbe { _ = store.unavailable }),
            ("isLoading", ObservationProbe { _ = store.isLoading }),
            ("error", ObservationProbe { _ = store.error }),
        ]
    }

    func changed() -> [String] { probes.filter { $0.1.consume() }.map(\.0) }
}
