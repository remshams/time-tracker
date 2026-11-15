import XCTest
@testable import TrackerClient

final class TrackingPillTests: XCTestCase {
    @MainActor
    func testRunningTaskShowsItsNameElapsedTimeAndStopAction() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        fixture.features.taskCatalog.select(secondTask.id)
        let pill = fixture.features.trackingPill

        let shown = try XCTUnwrap(pill.shown)
        XCTAssertEqual(shown.mode, .running(worklogID: activeWorklog.id, elapsedSeconds: 30))
        XCTAssertEqual(shown.taskID, firstTask.id)
        XCTAssertEqual(pill.taskID, firstTask.id)
        XCTAssertEqual(shown.name, "First task")
        XCTAssertTrue(shown.isRunning)
        XCTAssertEqual(shown.elapsed, DurationText(seconds: 30))
        XCTAssertEqual(shown.actionLabel, "Stop tracking")
        XCTAssertTrue(pill.isEnabled)
        XCTAssertFalse(pill.isStale)
        XCTAssertFalse(pill.isInFlight)
        XCTAssertEqual(pill.help, "Stop tracking First task")
        XCTAssertEqual(pill.accessibilityValue, "First task, 30 seconds")
    }

    @MainActor
    func testIdlePillOffersTheSelectedTask() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        fixture.features.taskCatalog.select(secondTask.id)
        let pill = fixture.features.trackingPill

        let shown = try XCTUnwrap(pill.shown)
        XCTAssertEqual(shown.mode, .idle)
        XCTAssertEqual(shown.taskID, secondTask.id)
        XCTAssertEqual(pill.taskID, secondTask.id)
        XCTAssertFalse(shown.isRunning)
        XCTAssertNil(shown.elapsed)
        XCTAssertEqual(shown.actionLabel, "Start tracking")
        XCTAssertTrue(pill.isEnabled)
        XCTAssertEqual(pill.help, "Start tracking Second task")
        XCTAssertEqual(pill.accessibilityValue, "Second task")
    }

    @MainActor
    func testNothingRunsWithoutASelectedActiveTask() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [archivedTask], active: nil))
        let pill = fixture.features.trackingPill
        XCTAssertNil(pill.shown)
        fixture.features.taskCatalog.changeTab(.archived)
        XCTAssertEqual(fixture.features.taskCatalog.selectedTaskID, archivedTask.id)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()

        XCTAssertNil(pill.shown)
        XCTAssertNil(pill.taskID)
        XCTAssertFalse(pill.isEnabled)
        XCTAssertNil(pill.help)
        XCTAssertNil(pill.accessibilityValue)
        let operationCount = fixture.client.operations.count
        pill.click()
        XCTAssertEqual(fixture.client.operations.count, operationCount)
    }

    @MainActor
    func testRunningTaskMissingFromTheCatalogReadsAsUnknown() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [secondTask], active: activeWorklog))

        XCTAssertEqual(fixture.features.trackingPill.shown?.name, "Unknown task")
    }

    @MainActor
    func testStaleConnectionDisablesThePillAndNotesItInTheTooltipAndValue() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: activeWorklog))
        fixture.sync.refresh()
        let refresh = try await fixture.client.next()
        refresh.fail(BridgeFailure(message: "Tracker offline", kind: "unavailable"))
        try await fixture.settled()
        let pill = fixture.features.trackingPill

        XCTAssertTrue(pill.isStale)
        XCTAssertFalse(pill.isEnabled)
        XCTAssertEqual(pill.help, "Stop tracking First task. The connection is stale.")
        XCTAssertEqual(pill.accessibilityValue, "First task, 30 seconds, connection stale")
    }

    @MainActor
    func testElapsedTimeIsMissingBeforeTheTrackerStateIsConfirmed() async {
        let store = TrackingStore()
        store.acceptTaskList(active: activeWorklog)

        XCTAssertNil(store.elapsedSeconds)
        XCTAssertNil(
            TrackingPillState(
                mode: .running(worklogID: "worklog", elapsedSeconds: nil), taskID: "task",
                name: "Task"
            ).elapsed)
    }

    @MainActor
    func testStopKeepsTheClickedStateWhileInFlightAndAnnouncesTheStop() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        fixture.features.taskCatalog.select(secondTask.id)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        let pill = fixture.features.trackingPill
        var announcements: [String] = []
        pill.announce = { announcements.append($0) }
        let running = pill.shown

        pill.click()

        XCTAssertTrue(pill.isInFlight)
        XCTAssertFalse(pill.isEnabled)
        let stop = try await fixture.client.next()
        XCTAssertEqual(stop.operation, .stop(worklog: activeWorklog.id, at: "2025-01-01T00:00:00.000Z"))
        // A background reload lands while the stop is in flight.
        fixture.features.tracking.acceptTaskList(active: nil)
        XCTAssertEqual(pill.shown, running, "The pill must keep the clicked state until the request ends.")
        XCTAssertEqual(pill.taskID, firstTask.id)
        XCTAssertEqual(announcements, [])
        stop.succeed(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        try await fixture.settled()
        try await fixture.waitUntil("finished stop") { !pill.isInFlight }

        XCTAssertEqual(pill.shown, TrackingPillState(mode: .idle, taskID: secondTask.id, name: "Second task"))
        XCTAssertEqual(pill.taskID, secondTask.id)
        XCTAssertEqual(announcements, ["Stopped First task."])
    }

    @MainActor
    func testFailedStopIsNotAnnouncedAndReleasesTheClickedState() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: activeWorklog))
        let pill = fixture.features.trackingPill
        var announcements: [String] = []
        pill.announce = { announcements.append($0) }

        pill.click()
        let stop = try await fixture.client.next()
        stop.fail(BridgeFailure(message: "Stop rejected", kind: "conflict"))
        let reload = try await fixture.client.next()
        reload.succeed(TaskListResources(tasks: [firstTask], active: activeWorklog))
        try await fixture.settled()
        try await fixture.waitUntil("finished stop") { !pill.isInFlight }

        XCTAssertNotNil(fixture.features.tracking.error)
        XCTAssertEqual(announcements, [])
        XCTAssertEqual(pill.shown?.mode, .running(worklogID: activeWorklog.id, elapsedSeconds: 30))
    }

    @MainActor
    func testStopThatLeavesAnotherWorklogRunningIsNotAnnounced() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        let pill = fixture.features.trackingPill
        var announcements: [String] = []
        pill.announce = { announcements.append($0) }
        let other = WorklogItem(id: "worklog-other", taskId: secondTask.id, start: activeWorklog.start, end: nil)

        pill.click()
        let stop = try await fixture.client.next()
        stop.succeed(TaskListResources(tasks: [firstTask, secondTask], active: other))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        try await fixture.waitUntil("finished stop") { !pill.isInFlight }

        XCTAssertNil(fixture.features.tracking.error)
        XCTAssertEqual(announcements, [])
        XCTAssertEqual(pill.shown?.taskID, secondTask.id)
    }

    @MainActor
    func testStartKeepsTheClickedStateWhileInFlightWithoutAnnouncing() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        let pill = fixture.features.trackingPill
        var announcements: [String] = []
        pill.announce = { announcements.append($0) }
        let idle = pill.shown

        pill.click()

        let start = try await fixture.client.next()
        XCTAssertEqual(
            start.operation, .start(task: firstTask.id, expected: nil, at: "2025-01-01T00:00:00.000Z"))
        fixture.features.tracking.acceptTaskList(active: activeWorklog)
        XCTAssertEqual(pill.shown, idle, "The pill must keep the clicked state until the request ends.")
        start.succeed(TaskListResources(tasks: [firstTask], active: activeWorklog))
        try await fixture.settled()
        try await fixture.waitUntil("finished start") { !pill.isInFlight }

        XCTAssertEqual(pill.shown?.mode, .running(worklogID: activeWorklog.id, elapsedSeconds: 30))
        XCTAssertEqual(announcements, [])
    }

    @MainActor
    func testRequestFromElsewhereShowsTheCurrentState() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        let pill = fixture.features.trackingPill
        var announcements: [String] = []
        pill.announce = { announcements.append($0) }

        fixture.features.tracking.stopTracking(worklogID: activeWorklog.id)
        let stop = try await fixture.client.next()
        fixture.features.tracking.acceptTaskList(active: nil)

        XCTAssertTrue(pill.isInFlight)
        XCTAssertEqual(pill.shown?.mode, .idle)
        XCTAssertEqual(pill.taskID, firstTask.id)
        stop.succeed(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        try await fixture.settled()
        XCTAssertEqual(announcements, [])
    }

    @MainActor
    func testClickThatCannotSendLeavesNothingFrozen() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        let pill = fixture.features.trackingPill
        fixture.sync.refresh()
        let refresh = try await fixture.client.next()
        refresh.fail(BridgeFailure(message: "Tracker offline", kind: "unavailable"))
        try await fixture.settled()
        let operationCount = fixture.client.operations.count

        pill.click()

        XCTAssertEqual(fixture.client.operations.count, operationCount)
        // A later request from elsewhere must show its own state, not the click that sent nothing.
        fixture.sync.refresh()
        let recovery = try await fixture.client.next()
        recovery.succeed(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        try await fixture.settled()
        fixture.features.taskCatalog.select(secondTask.id)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        fixture.features.tracking.startTracking(taskID: secondTask.id)
        let start = try await fixture.client.next()
        XCTAssertTrue(pill.isInFlight)
        XCTAssertEqual(pill.shown?.taskID, secondTask.id)
        start.succeed(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        try await fixture.settled()
    }

    @MainActor
    func testShowTaskSelectsTheRunningTaskAndChangesToItsTab() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: activeWorklog))
        let catalog = fixture.features.taskCatalog
        catalog.changeTab(.archived)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()

        fixture.features.trackingPill.showTask()

        XCTAssertEqual(catalog.tab, .active)
        XCTAssertEqual(catalog.selectedTaskID, firstTask.id)
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .history(task: firstTask.id, cursor: nil))
        reload.succeed(emptyPage)
        try await fixture.settled()
    }

    @MainActor
    func testShowTaskDoesNothingWhileNothingRuns() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [archivedTask], active: nil))
        let operationCount = fixture.client.operations.count

        fixture.features.trackingPill.showTask()

        XCTAssertEqual(fixture.features.taskCatalog.tab, .active)
        XCTAssertEqual(fixture.client.operations.count, operationCount)
    }

    @MainActor
    func testClockTicksRedrawTheShownTimeButNotTheTaskID() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        fixture.sync.setWindowVisible(true)
        try await fixture.start(TaskListResources(tasks: [firstTask], active: activeWorklog))
        let pill = fixture.features.trackingPill
        let shown = ObservationProbe { _ = pill.shown }
        let taskID = ObservationProbe { _ = pill.taskID }
        let display = try XCTUnwrap(fixture.scheduler.display)

        fixture.advanceClock(by: 1)
        display.fire()

        XCTAssertTrue(shown.consume())
        XCTAssertEqual(pill.shown?.elapsed?.text, "0:00:31")
        XCTAssertFalse(taskID.consume(), "A sidebar row reads the task ID and must not redraw every second.")
    }
}
