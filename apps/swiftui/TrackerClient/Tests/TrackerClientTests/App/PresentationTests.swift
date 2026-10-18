import XCTest
@testable import TrackerClient

final class PresentationTests: XCTestCase {
    @MainActor
    func testEditorAvailabilityNotifiesOnlyWhenTheAvailabilityChanges() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let harness = PresentationHarness(session: fixture.session)
        let bulk = harness.counter(PresentationReads.bulkTaskArchiving)
        let move = harness.counter(PresentationReads.worklogMove)
        let correction = harness.counter(PresentationReads.worklogCorrection)
        let deletion = harness.counter(PresentationReads.worklogDeletion)

        harness.update()
        XCTAssertEqual([bulk.count, move.count, correction.count, deletion.count], [0, 0, 0, 0])
        XCTAssertFalse(harness.presentation.canOpenWorklogDeletion)
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        harness.attach()
        harness.update()
        XCTAssertEqual([bulk.count, move.count, correction.count, deletion.count], [1, 1, 1, 1])
        XCTAssertTrue(harness.presentation.canOpenWorklogDeletion)
        fixture.session.openTaskCreation()
        XCTAssertEqual([bulk.count, move.count, correction.count, deletion.count], [1, 1, 1, 1])
        fixture.session.cancelTaskCreation()
        harness.update()
        XCTAssertEqual([bulk.count, move.count, correction.count, deletion.count], [1, 1, 1, 1])
    }

    @MainActor
    func testChangedServerContentAndPreferencesStillNotifyTheWindow() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: activeWorklog))
        let harness = PresentationHarness(session: fixture.session)
        let names = harness.recorder(PresentationReads.content) { harness.presentation.selectedTask?.name }
        let renamed = TaskItem(id: firstTask.id, name: "Renamed task", archived: false, latestStart: nil)
        fixture.scheduler.poll?.fire()
        let refresh = try await fixture.client.next()
        refresh.succeed(TaskListResources(tasks: [renamed], active: activeWorklog))
        try await harness.settled(fixture)
        XCTAssertEqual(names.values, ["Renamed task"])
        fixture.session.setPauseOnScreenLock(true)
        XCTAssertEqual(names.values, ["Renamed task", "Renamed task"])
        XCTAssertTrue(harness.presentation.pauseOnScreenLock)
        harness.update()
        XCTAssertEqual(names.count, 2)
    }

    @MainActor
    func testClockTicksUpdateOnlyElapsedTextWithoutInvalidatingLists() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        fixture.session.setWindowVisible(true)
        try await fixture.start(TaskListResources(tasks: [firstTask], active: activeWorklog))
        let harness = PresentationHarness(session: fixture.session)
        let content = harness.counter(PresentationReads.content)
        let controls = harness.counter(PresentationReads.controls)
        let timer = harness.recorder(PresentationReads.timer) { harness.presentation.timerDisplayText }
        let display = try XCTUnwrap(fixture.scheduler.display)
        let operationCount = fixture.client.operations.count

        for _ in 0..<3 {
            fixture.clock.now.addTimeInterval(1)
            fixture.clock.uptime += 1
            display.fire()
        }
        XCTAssertEqual(timer.values, ["00:00:31", "00:00:32", "00:00:33"])
        XCTAssertEqual(content.count, 0)
        XCTAssertEqual(controls.count, 0)
        XCTAssertEqual(fixture.client.operations.count, operationCount)
        harness.update()
        XCTAssertEqual(timer.count, 3, "Repeating the same formatted second must not redraw the timer.")
    }

    @MainActor
    func testUnchangedServerPollsDoNotInvalidateControlsOrLists() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(resources)
        let harness = PresentationHarness(session: fixture.session)
        let content = harness.counter(PresentationReads.content)
        let controls = harness.counter(PresentationReads.controls)
        let timer = harness.counter(PresentationReads.timer)

        for _ in 0..<3 {
            fixture.scheduler.poll?.fire()
            let refresh = try await fixture.client.next()
            XCTAssertTrue(fixture.session.isBusy)
            XCTAssertTrue(fixture.session.canStopTracking)
            XCTAssertFalse(fixture.session.isBlockingControls)
            refresh.succeed(resources)
            try await harness.settled(fixture)
            XCTAssertTrue(fixture.session.canStopTracking)
        }
        XCTAssertEqual(content.count, 0)
        XCTAssertEqual(timer.count, 0)
        XCTAssertEqual(controls.count, 0)
    }

    @MainActor
    func testDraftEditsNotifyOnlyTheirSheet() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        let harness = PresentationHarness(session: fixture.session)
        fixture.session.openTaskCreation()
        let lists = harness.counter(PresentationReads.lists)
        let controls = harness.counter(PresentationReads.controls)
        let rename = harness.counter(PresentationReads.taskRename)
        let creation = harness.recorder(PresentationReads.taskCreation) { harness.presentation.taskCreation.name }

        for name in ["N", "Ne", "New"] { fixture.session.setTaskCreationName(name) }
        XCTAssertEqual(creation.values, ["N", "Ne", "New"])
        XCTAssertEqual(lists.count, 0)
        XCTAssertEqual(controls.count, 0)
        XCTAssertEqual(rename.count, 0)
    }

    @MainActor
    func testNavigationAndTrackingUpdatePresentedValues() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: nil))
        let harness = PresentationHarness(session: fixture.session)
        let lists = harness.counter(PresentationReads.lists)
        let controls = harness.counter(PresentationReads.controls)

        fixture.session.select(secondTask.id)
        let history = try await fixture.client.next()
        XCTAssertTrue(harness.presentation.isHistoryLoading)
        history.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "older", reset: false))
        try await harness.settled(fixture)
        XCTAssertEqual(harness.presentation.selectedTask, secondTask)
        XCTAssertEqual(harness.presentation.worklogs, [oldWorklog])
        XCTAssertTrue(harness.presentation.hasMoreHistory)

        fixture.session.changeTab(.archived)
        XCTAssertEqual(harness.presentation.tab, .archived)
        XCTAssertEqual(harness.presentation.visibleTasks, [archivedTask])
        XCTAssertEqual(harness.presentation.selectedTask, archivedTask)
        try await fixture.client.next().succeed(emptyPage)
        try await harness.settled(fixture)
        XCTAssertTrue(harness.presentation.canUnarchiveTask(taskID: archivedTask.id))
        XCTAssertFalse(harness.presentation.canArchiveTask(taskID: archivedTask.id))
        fixture.session.changeTab(.active)
        try await fixture.client.next().succeed(emptyPage)
        try await harness.settled(fixture)
        XCTAssertTrue(harness.presentation.canArchiveTask(taskID: firstTask.id))
        XCTAssertGreaterThan(lists.count, 0)

        XCTAssertTrue(harness.presentation.canStartTracking(taskID: firstTask.id))
        fixture.session.startTracking(taskID: firstTask.id)
        XCTAssertTrue(harness.presentation.isBlockingControls)
        XCTAssertFalse(harness.presentation.canStartTracking(taskID: firstTask.id))
        let start = try await fixture.client.next()
        start.succeed(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: activeWorklog))
        try await harness.settled(fixture)
        XCTAssertEqual(harness.presentation.active, activeWorklog)
        XCTAssertTrue(harness.presentation.canStopTracking)
        XCTAssertFalse(harness.presentation.canArchiveTask(taskID: firstTask.id))
        XCTAssertGreaterThan(controls.count, 0)
    }

    @MainActor
    func testFailuresAndConnectionChangesUpdatePresentedValues() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        let harness = PresentationHarness(session: fixture.session)

        fixture.session.select(secondTask.id)
        let history = try await fixture.client.next()
        history.fail(BridgeFailure(message: "History unavailable", kind: "unavailable"))
        try await harness.settled(fixture)
        XCTAssertNotNil(harness.presentation.error)
        XCTAssertTrue(harness.presentation.historyUnavailable)

        let connecting = Task { await fixture.session.connect(serverSettings) }
        let request = try await fixture.client.next()
        XCTAssertTrue(harness.presentation.isChangingConnection)
        request.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        _ = try await fixture.taskValue(connecting)
        try await harness.settled(fixture)
        XCTAssertFalse(harness.presentation.isChangingConnection)
        XCTAssertNotNil(harness.presentation.connectionMessage)

        let connected = Task { await fixture.session.connect(serverSettings) }
        let accepted = try await fixture.client.next()
        accepted.succeed(TaskListResources(tasks: [firstTask], active: nil))
        try await fixture.client.next().succeed(emptyPage)
        let didConnect = try await fixture.taskValue(connected)
        XCTAssertTrue(didConnect)
        try await harness.settled(fixture)
        XCTAssertEqual(harness.presentation.connectionSettings, serverSettings)
    }

    @MainActor
    func testTrackingErrorsAndAutomaticPauseUpdatePresentedValues() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        let running = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(running)
        let harness = PresentationHarness(session: fixture.session)

        fixture.session.startTracking(taskID: secondTask.id)
        let write = try await fixture.client.next()
        write.fail(BridgeFailure(message: "Response lost", kind: "unavailable", uncertain: true))
        let confirmation = try await fixture.client.next()
        confirmation.succeed(running)
        try await harness.settled(fixture)
        XCTAssertEqual(harness.presentation.trackingError, "Response lost")
        fixture.session.dismissTrackingError()
        XCTAssertNil(harness.presentation.trackingError)

        fixture.session.screenLocked(at: fixture.clock.now)
        let taskList = try await fixture.client.next()
        taskList.succeed(running)
        let pause = try await fixture.client.next()
        pause.paused(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await harness.settled(fixture)
        XCTAssertEqual(harness.presentation.autoPauseStatusText, "Paused while screen is locked")
        XCTAssertNil(harness.presentation.active)
    }
}
