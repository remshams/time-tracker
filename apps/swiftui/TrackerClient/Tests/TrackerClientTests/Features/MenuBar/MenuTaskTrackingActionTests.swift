import XCTest
@testable import TrackerClient

final class MenuTaskTrackingActionTests: XCTestCase {
    @MainActor
    func testIdleTaskStartsAndRunningTaskStopsItsCapturedWorklog() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog))
        let content = TrackerMenuContent(fixture.session)
        let running = try XCTUnwrap((content.todayTasks + content.otherTasks).first { $0.id == firstTask.id })
        let idle = try XCTUnwrap(content.otherTasks.first { $0.id == secondTask.id })
        XCTAssertEqual(running.trackingAction(in: content), .stop(activeWorklog.id))
        XCTAssertEqual(idle.trackingAction(in: content), .start(secondTask.id))
    }

    @MainActor
    func testBusyRunningTaskCannotFallBackToStarting() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog))
        fixture.session.startTracking(taskID: secondTask.id)
        let content = TrackerMenuContent(fixture.session)
        XCTAssertFalse(content.canStopTracking)
        let running = TrackerMenuTask(task: firstTask, durationText: "-", isRunning: true, canStart: true)
        XCTAssertNil(running.trackingAction(in: content))
        let idle = try XCTUnwrap(content.otherTasks.first { $0.id == secondTask.id })
        XCTAssertFalse(idle.canStart)
        XCTAssertNil(idle.trackingAction(in: content))
    }

    @MainActor
    func testRunningTaskWithoutAnActiveWorklogCannotFallBackToStarting() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask], active: nil))
        let content = TrackerMenuContent(fixture.session)
        XCTAssertNil(content.activeWorklogID)
        let running = TrackerMenuTask(task: firstTask, durationText: "-", isRunning: true, canStart: true)
        XCTAssertNil(running.trackingAction(in: content))
    }

    @MainActor
    func testStaleMenuCannotStartOrStopTracking() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog))
        fixture.scheduler.poll?.fire()
        let poll = try await fixture.client.next()
        poll.fail(BridgeFailure(message: "Server unavailable", kind: "unavailable"))
        try await fixture.settled()
        let content = TrackerMenuContent(fixture.session)
        XCTAssertTrue(content.isStale)
        let running = TrackerMenuTask(task: firstTask, durationText: "-", isRunning: true, canStart: true)
        let idle = TrackerMenuTask(task: secondTask, durationText: "-", isRunning: false, canStart: true)
        XCTAssertNil(running.trackingAction(in: content))
        XCTAssertNil(idle.trackingAction(in: content))
    }

    @MainActor
    func testArchivedTaskRemainsCopyableAndCannotStartEvenWithAnOutdatedCapability() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [archivedTask], active: nil),
                                rows: [TaskReportTotal(taskId: archivedTask.id, durationMicroseconds: 60_000_000)])
        let content = TrackerMenuContent(fixture.session)
        let entry = try XCTUnwrap(content.todayTasks.first)
        XCTAssertNil(entry.trackingAction(in: content))
        let outdated = TrackerMenuTask(task: archivedTask, durationText: entry.durationText,
                                      isRunning: false, canStart: true)
        XCTAssertNil(outdated.trackingAction(in: content))
        XCTAssertEqual(fixture.session.menuCopyValue(.copyName, taskID: archivedTask.id, connection: .local),
                       archivedTask.name)
        XCTAssertEqual(fixture.session.menuCopyValue(.copyExact, taskID: archivedTask.id, connection: .local), "1m 0s")
        XCTAssertNil(fixture.session.active)
    }

    @MainActor
    func testFrozenStopActionKeepsItsWorklogIdentityAndTheSessionRejectsAChangedWorklog() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask], active: activeWorklog))
        let observer = TrackerMenuPresentationObserver(session: fixture.session, showDailyTotal: true)
        fixture.session.onChange = { observer.update(from: fixture.session, showDailyTotal: true) }
        observer.menuOpened(from: fixture.session, showDailyTotal: true)
        let captured = observer.content
        let entry = try XCTUnwrap((captured.todayTasks + captured.otherTasks).first)
        let changed = WorklogItem(id: "worklog-from-other-client", taskId: firstTask.id,
                                 start: activeWorklog.start, end: nil)
        fixture.scheduler.poll?.fire()
        let poll = try await fixture.client.next()
        poll.succeed(TrackerSnapshot(tasks: [firstTask], active: changed))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(entry.trackingAction(in: observer.content), .stop(activeWorklog.id))
        guard case .stop(let id) = try XCTUnwrap(entry.trackingAction(in: captured)) else {
            return XCTFail("Expected the captured stop action")
        }
        let count = fixture.client.operations.count
        fixture.session.stopTracking(worklogID: id)
        XCTAssertEqual(fixture.client.operations.count, count)
        XCTAssertEqual(fixture.session.active?.id, changed.id)
        observer.menuClosed(from: fixture.session, showDailyTotal: true)
        XCTAssertEqual(entry.trackingAction(in: observer.content), .stop(changed.id))
    }
}
