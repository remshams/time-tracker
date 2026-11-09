import XCTest
@testable import TrackerClient

final class HistoryTests: XCTestCase {
    @MainActor
    func testChangingTheTabOrTaskLoadsTheHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: activeWorklog))

        fixture.features.taskCatalog.select(secondTask.id)
        XCTAssertEqual(fixture.features.taskCatalog.selectedTaskID, secondTask.id)
        let selected = try await fixture.client.next()
        XCTAssertEqual(selected.operation, .history(task: secondTask.id, cursor: nil))
        selected.succeed(emptyPage)
        try await fixture.settled()

        fixture.features.taskCatalog.changeTab(.archived)
        XCTAssertEqual(fixture.features.taskCatalog.tab, .archived)
        let changed = try await fixture.client.next()
        XCTAssertEqual(changed.operation, .history(task: archivedTask.id, cursor: nil))
        changed.succeed(emptyPage)
        try await fixture.settled()
    }

    @MainActor
    func testNavigationUpdatesTheListsAndTheHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: nil))
        let lists = ObservationProbe { ViewReads.lists(fixture.features) }
        let catalog = fixture.features.taskCatalog
        let worklogHistory = fixture.features.worklogHistory

        catalog.select(secondTask.id)
        let history = try await fixture.client.next()
        XCTAssertTrue(worklogHistory.isLoading)
        XCTAssertTrue(lists.consume())
        history.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "older", reset: false))
        try await fixture.settled()
        XCTAssertEqual(catalog.selectedTask, secondTask)
        XCTAssertEqual(worklogHistory.worklogs, [oldWorklog])
        XCTAssertTrue(worklogHistory.hasMore)
        XCTAssertTrue(lists.consume())

        catalog.changeTab(.archived)
        XCTAssertEqual(catalog.tab, .archived)
        XCTAssertEqual(catalog.visibleTasks, [archivedTask])
        XCTAssertEqual(catalog.selectedTask, archivedTask)
        XCTAssertTrue(lists.consume())
        try await fixture.client.next().succeed(emptyPage)
        try await fixture.settled()
        catalog.changeTab(.active)
        XCTAssertEqual(catalog.selectedTask, secondTask)
        XCTAssertTrue(lists.consume())
        try await fixture.client.next().succeed(emptyPage)
        try await fixture.settled()
    }

    @MainActor
    func testKeepingTheTabOrTaskSendsNothing() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))

        fixture.features.taskCatalog.select(firstTask.id)
        fixture.features.taskCatalog.changeTab(.active)

        XCTAssertFalse(fixture.client.hasPendingRequests)
    }

    @MainActor
    func testSelectingAnotherTaskShowsLoadingWithoutPreviousRowsOrCursor() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        fixture.features.worklogHistory.reload()
        let initial = try await fixture.client.next()
        initial.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "older", reset: false))
        try await fixture.settled()

        fixture.features.taskCatalog.select(secondTask.id)
        XCTAssertTrue(fixture.features.worklogHistory.isLoading)
        XCTAssertTrue(fixture.features.worklogHistory.worklogs.isEmpty)
        XCTAssertNil(fixture.features.worklogHistory.nextCursor)
        let selected = try await fixture.client.next()
        XCTAssertEqual(selected.operation, .history(task: secondTask.id, cursor: nil))
        selected.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(fixture.features.worklogHistory.isLoading)
    }

    @MainActor
    func testPollingKeepsExistingRowsWhileSameTaskHistoryRefreshes() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        let running = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(running)
        fixture.features.worklogHistory.reload()
        let initial = try await fixture.client.next()
        initial.succeed(HistoryPage(worklogs: [activeWorklog, oldWorklog], nextCursor: "older", reset: false))
        try await fixture.settled()

        fixture.scheduler.poll?.fire()
        let refresh = try await fixture.client.next()
        refresh.succeed(TaskListResources(tasks: [firstTask], active: nil))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        XCTAssertEqual(
            fixture.features.worklogHistory.worklogs, [activeWorklog, oldWorklog],
            "Existing rows must remain visible while their replacement is fetched.")
        XCTAssertEqual(fixture.features.worklogHistory.nextCursor, "older")
        XCTAssertTrue(fixture.features.worklogHistory.isLoading)
        XCTAssertTrue(fixture.features.worklogHistory.hasMore)
        fixture.features.worklogHistory.loadOlder()
        history.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertEqual(fixture.features.worklogHistory.worklogs, [oldWorklog])
        XCTAssertNil(fixture.features.worklogHistory.nextCursor)
    }

    @MainActor
    func testRefreshingHistoryRetainsPaginationButRejectsLoadOlderUntilConfirmed() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.features.worklogHistory.reload()
        let initial = try await fixture.client.next()
        let page = HistoryPage(worklogs: [oldWorklog], nextCursor: "older", reset: false)
        initial.succeed(page)
        try await fixture.settled()
        fixture.features.worklogHistory.reload()
        let refresh = try await fixture.client.next()
        XCTAssertEqual(fixture.features.worklogHistory.nextCursor, "older")
        XCTAssertTrue(fixture.features.worklogHistory.hasMore)
        XCTAssertTrue(fixture.features.worklogHistory.isLoading)
        fixture.features.worklogHistory.loadOlder()
        refresh.succeed(page)
        try await fixture.settled()
        XCTAssertFalse(fixture.features.worklogHistory.isLoading)
        XCTAssertTrue(fixture.features.worklogHistory.hasMore)
        fixture.features.worklogHistory.loadOlder()
        let older = try await fixture.client.next()
        XCTAssertEqual(older.operation, .history(task: firstTask.id, cursor: "older"))
        older.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(fixture.features.worklogHistory.hasMore)
    }

    @MainActor
    func testIncompatibleHistoryDisablesTrackingAndStopsAutomaticPolling() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        XCTAssertTrue(fixture.canStartSelectedTask)
        XCTAssertNotNil(fixture.scheduler.poll)
        fixture.features.worklogHistory.reload()
        let history = try await fixture.client.next()
        history.fail(BridgeFailure(message: "Unsupported history version", kind: "protocol"))
        try await fixture.settled()

        XCTAssertTrue(fixture.features.worklogHistory.unavailable)
        XCTAssertTrue(fixture.features.connection.stale)
        XCTAssertFalse(fixture.canStartSelectedTask)
        XCTAssertEqual(fixture.features.connection.statusText, "Incompatible server")
        XCTAssertEqual(fixture.features.connection.message, "Unsupported history version")
        XCTAssertNil(fixture.scheduler.poll)
    }

    @MainActor
    func testHistoryRequiringRefreshDisablesTrackingUntilResourcesAreConfirmed() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources)
        XCTAssertTrue(fixture.canStartSelectedTask)
        fixture.features.worklogHistory.reload()
        let history = try await fixture.client.next()
        history.fail(BridgeFailure(message: "Tracker state must be refreshed", requiresRefresh: true))
        try await fixture.settled()

        XCTAssertTrue(fixture.features.worklogHistory.unavailable)
        XCTAssertTrue(fixture.features.connection.stale)
        XCTAssertFalse(fixture.canStartSelectedTask)
        XCTAssertEqual(fixture.features.connection.statusText, "Unavailable")
        XCTAssertEqual(fixture.features.connection.message, "Tracker state must be refreshed")

        fixture.sync.refresh()
        let refresh = try await fixture.client.next()
        XCTAssertFalse(fixture.canStartSelectedTask)
        refresh.succeed(resources)
        let retry = try await fixture.client.next()
        retry.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(fixture.features.connection.stale)
        XCTAssertFalse(fixture.features.worklogHistory.unavailable)
        XCTAssertTrue(fixture.canStartSelectedTask)
    }

    @MainActor
    func testSelectionsAreRememberedPerTabAndRemovedSelectionFallsBack() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: nil)
        try await fixture.start(resources)
        fixture.features.taskCatalog.select(secondTask.id)
        let selected = try await fixture.client.next()
        selected.succeed(emptyPage)
        try await fixture.settled()
        fixture.features.taskCatalog.changeTab(.archived)
        let archived = try await fixture.client.next()
        XCTAssertEqual(archived.operation, .history(task: archivedTask.id, cursor: nil))
        archived.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.features.taskCatalog.visibleTasks, [archivedTask])
        fixture.features.taskCatalog.select(firstTask.id)
        XCTAssertEqual(fixture.features.taskCatalog.selectedTaskID, archivedTask.id)
        fixture.features.taskCatalog.changeTab(.active)
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
        XCTAssertEqual(fixture.features.taskCatalog.selectedTaskID, firstTask.id)
    }

    @MainActor
    func testDelayedHistoryCannotReplaceNewSelectionOrItsErrorState() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        fixture.features.worklogHistory.reload()
        let oldRequest = try await fixture.client.next()
        fixture.features.taskCatalog.select(secondTask.id)
        XCTAssertTrue(fixture.features.worklogHistory.isLoading)
        XCTAssertTrue(fixture.features.worklogHistory.worklogs.isEmpty)
        oldRequest.fail(BridgeFailure(message: "Old selection failed", kind: "unavailable"))
        let newRequest = try await fixture.client.next()
        XCTAssertEqual(newRequest.operation, .history(task: secondTask.id, cursor: nil))
        XCTAssertFalse(fixture.features.worklogHistory.unavailable)
        XCTAssertTrue(fixture.features.worklogHistory.isLoading)
        XCTAssertNil(fixture.features.worklogHistory.error)
        XCTAssertFalse(fixture.features.connection.stale)
        newRequest.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.features.taskCatalog.selectedTaskID, secondTask.id)
        XCTAssertFalse(fixture.features.worklogHistory.unavailable)
        XCTAssertNil(fixture.features.worklogHistory.error)
    }

    @MainActor
    func testDelayedSuccessfulHistoryCannotPopulateNewSelection() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        fixture.features.worklogHistory.reload()
        let oldRequest = try await fixture.client.next()
        fixture.features.taskCatalog.select(secondTask.id)
        oldRequest.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "obsolete", reset: false))
        let newRequest = try await fixture.client.next()
        XCTAssertTrue(fixture.features.worklogHistory.worklogs.isEmpty)
        XCTAssertNil(fixture.features.worklogHistory.nextCursor)
        newRequest.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertTrue(fixture.features.worklogHistory.worklogs.isEmpty)
    }

    @MainActor
    func testReturningToTaskAcceptsItsEarlierHistoryRequest() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        fixture.features.worklogHistory.reload()
        let earlier = try await fixture.client.next()
        fixture.features.taskCatalog.select(secondTask.id)
        let other = try await fixture.client.next()
        fixture.features.taskCatalog.select(firstTask.id)
        let current = try await fixture.client.next()
        try await fixture.update("the current page") { current.succeed(emptyPage) }
        try await fixture.update("the earlier page for the selected task") {
            earlier.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "older", reset: false))
        }
        XCTAssertEqual(fixture.features.worklogHistory.worklogs, [oldWorklog])
        XCTAssertEqual(fixture.features.worklogHistory.nextCursor, "older")
        other.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.features.worklogHistory.worklogs, [oldWorklog])
        XCTAssertFalse(fixture.features.connection.stale)
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 3)
    }

    @MainActor
    func testPaginationAppendsButResetReplacesPreviousRowsAndCursor() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.features.worklogHistory.reload()
        let first = try await fixture.client.next()
        first.succeed(HistoryPage(worklogs: [activeWorklog], nextCursor: "page-two", reset: false))
        try await fixture.settled()
        XCTAssertTrue(fixture.features.worklogHistory.hasMore)
        fixture.features.worklogHistory.loadOlder()
        let older = try await fixture.client.next()
        XCTAssertEqual(older.operation, .history(task: firstTask.id, cursor: "page-two"))
        XCTAssertTrue(fixture.features.worklogHistory.isLoading)
        XCTAssertTrue(fixture.features.worklogHistory.hasMore)
        fixture.features.worklogHistory.loadOlder()
        older.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "page-three", reset: false))
        try await fixture.settled()
        XCTAssertEqual(fixture.features.worklogHistory.worklogs, [activeWorklog, oldWorklog])
        fixture.features.worklogHistory.loadOlder()
        let reset = try await fixture.client.next()
        XCTAssertEqual(reset.operation, .history(task: firstTask.id, cursor: "page-three"))
        reset.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: nil, reset: true))
        try await fixture.settled()
        XCTAssertEqual(fixture.features.worklogHistory.worklogs, [oldWorklog])
        XCTAssertFalse(fixture.features.worklogHistory.hasMore)
    }

    @MainActor
    func testHistoryFailureRetriesCurrentSelectionAndClearsError() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.features.worklogHistory.reload()
        let failure = try await fixture.client.next()
        failure.fail(BridgeFailure(message: "History read failed"))
        try await fixture.settled()
        XCTAssertTrue(fixture.features.worklogHistory.unavailable)
        XCTAssertFalse(fixture.features.worklogHistory.isLoading)
        XCTAssertEqual(fixture.features.worklogHistory.error, "History read failed")
        XCTAssertFalse(fixture.features.connection.stale)
        fixture.features.worklogHistory.reload()
        let retry = try await fixture.client.next()
        XCTAssertTrue(fixture.features.worklogHistory.isLoading)
        XCTAssertEqual(retry.operation, .history(task: firstTask.id, cursor: nil))
        retry.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertEqual(fixture.features.worklogHistory.worklogs, [oldWorklog])
        XCTAssertFalse(fixture.features.worklogHistory.unavailable)
        XCTAssertNil(fixture.features.worklogHistory.error)
    }

    @MainActor
    func testLoadingAnOlderPageAfterAFailedReloadClearsTheError() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.features.worklogHistory.reload()
        try await fixture.client.next().succeed(
            HistoryPage(worklogs: [activeWorklog], nextCursor: "older", reset: false))
        try await fixture.settled()
        fixture.features.worklogHistory.reload()
        try await fixture.client.next().fail(BridgeFailure(message: "History read failed"))
        try await fixture.settled()
        XCTAssertTrue(fixture.features.worklogHistory.unavailable)

        fixture.features.worklogHistory.loadOlder()
        let older = try await fixture.client.next()
        XCTAssertEqual(older.operation, .history(task: firstTask.id, cursor: "older"))
        older.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertEqual(fixture.features.worklogHistory.worklogs, [activeWorklog, oldWorklog])
        XCTAssertFalse(fixture.features.worklogHistory.unavailable)
        XCTAssertNil(fixture.features.worklogHistory.error)
    }

    @MainActor
    func testUnavailableHistoryMarksResourcesStaleAndRefreshRetriesHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources)
        fixture.features.worklogHistory.reload()
        let history = try await fixture.client.next()
        history.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertTrue(fixture.features.connection.stale)
        XCTAssertFalse(fixture.canStartSelectedTask)
        fixture.sync.refresh()
        let refresh = try await fixture.client.next()
        refresh.succeed(resources)
        let retry = try await fixture.client.next()
        XCTAssertTrue(fixture.features.worklogHistory.isLoading)
        XCTAssertEqual(retry.operation, .history(task: firstTask.id, cursor: nil))
        retry.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(fixture.features.connection.stale)
        XCTAssertFalse(fixture.features.worklogHistory.unavailable)
        XCTAssertTrue(fixture.canStartSelectedTask)
    }

    @MainActor
    func testPageForAnotherTaskNeverReachesTheHistoryReaders() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        fixture.features.worklogHistory.reload()
        let oldRequest = try await fixture.client.next()
        fixture.features.taskCatalog.select(secondTask.id)
        let probes = HistoryProbes(fixture.features.worklogHistory)

        oldRequest.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "obsolete", reset: false))
        let newRequest = try await fixture.client.next()
        newRequest.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(probes.changed(), ["isLoading"])
        XCTAssertEqual(fixture.features.worklogHistory.worklogs, [])
    }

    @MainActor
    func testUnchangedPollNotifiesNoHistoryReader() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources)
        let probes = HistoryProbes(fixture.features.worklogHistory)

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
        fixture.features.worklogHistory.reload()
        try await fixture.client.next().succeed(
            HistoryPage(worklogs: [activeWorklog], nextCursor: "page-two", reset: false))
        try await fixture.settled()
        let probes = HistoryProbes(fixture.features.worklogHistory)

        fixture.features.worklogHistory.loadOlder()
        XCTAssertEqual(probes.changed(), ["isLoading"])
        try await fixture.client.next().succeed(
            HistoryPage(worklogs: [oldWorklog], nextCursor: "page-three", reset: false))
        try await fixture.settled()
        XCTAssertEqual(probes.changed(), ["worklogs", "isLoading"])
        fixture.features.worklogHistory.loadOlder()
        XCTAssertEqual(probes.changed(), ["isLoading"])
        try await fixture.client.next().succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(probes.changed(), ["hasMore", "isLoading"])
        XCTAssertEqual(fixture.features.worklogHistory.worklogs, [activeWorklog, oldWorklog])
    }

    @MainActor
    func testConnectionFailureAndRecoveryNotifyOnlyErrorReaders() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources)
        let probes = HistoryProbes(fixture.features.worklogHistory)

        fixture.scheduler.poll?.fire()
        try await fixture.client.next().fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertEqual(probes.changed(), ["error"])
        XCTAssertEqual(fixture.features.worklogHistory.error, "Server offline")
        XCTAssertTrue(fixture.features.connection.stale)
        fixture.scheduler.poll?.fire()
        try await fixture.client.next().succeed(resources)
        try await fixture.settled()
        XCTAssertEqual(probes.changed(), ["error"])
        XCTAssertNil(fixture.features.worklogHistory.error)
        XCTAssertFalse(fixture.features.connection.stale)
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
