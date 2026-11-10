import Foundation
import XCTest
@testable import TrackerClient

final class BackgroundRefreshInteractionTests: XCTestCase {
    @MainActor
    func testTrackingSendsImmediatelyWhileAPollRunsAndDisablesOnlyTracking() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(resources)
        fixture.sync.refresh()
        let poll = try await fixture.client.next()
        fixture.features.tracking.startTracking(taskID: secondTask.id)
        fixture.clock.now.addTimeInterval(120)
        fixture.features.tracking.stopTracking(worklogID: activeWorklog.id)
        let start = try await fixture.client.next()
        XCTAssertEqual(
            start.operation,
            .start(task: secondTask.id, expected: activeWorklog.id, at: "2025-01-01T00:00:00.000Z"))
        XCTAssertFalse(fixture.trackingAvailability.canStart(taskID: secondTask.id))
        XCTAssertFalse(fixture.trackingAvailability.canStop)
        XCTAssertTrue(fixture.taskCreation.canOpen)
        fixture.taskCreation.open()
        fixture.taskCreation.setName("Independent task")
        fixture.taskCreation.submit()
        let create = try await fixture.client.next()
        XCTAssertEqual(create.operation, .create(name: "Independent task", at: "2025-01-01T00:02:00.000Z"))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 3)
        create.fail(BridgeFailure(message: "Name rejected"))
        let createReload = try await fixture.client.next()
        createReload.succeed(resources)
        poll.succeed(resources)
        start.succeed(resources)
        try await fixture.settled()
        XCTAssertTrue(fixture.trackingAvailability.canStart(taskID: secondTask.id))
        XCTAssertTrue(fixture.trackingAvailability.canStop)
        XCTAssertEqual(fixture.taskCreation.error, "Name rejected")
    }

    @MainActor
    func testStopSendsImmediatelyWhileHistoryLoadsAndKeepsClickTime() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(resources)
        fixture.features.worklogHistory.reload()
        let history = try await fixture.client.next()
        fixture.features.tracking.stopTracking(worklogID: activeWorklog.id)
        fixture.clock.now.addTimeInterval(90)
        fixture.features.tracking.stopTracking(worklogID: activeWorklog.id)
        let stop = try await fixture.client.next()
        XCTAssertEqual(stop.operation, .stop(worklog: activeWorklog.id, at: "2025-01-01T00:00:00.000Z"))
        XCTAssertFalse(fixture.trackingAvailability.canStop)
        stop.succeed(TaskListResources(tasks: [firstTask], active: nil))
        history.succeed(emptyPage)
        // The stopped worklog belonged to the selected task, so the reload requests its history again.
        let reloadedHistory = try await fixture.client.next()
        XCTAssertEqual(reloadedHistory.operation, .history(task: firstTask.id, cursor: nil))
        reloadedHistory.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertNil(fixture.features.tracking.active)
        XCTAssertTrue(fixture.canStartSelectedTask)
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .stop = $0 { return true }; return false
            }.count, 1)
    }

    @MainActor
    func testRefreshAndVisibilityChangesDoNotOverlapAnInflightPoll() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        let timer = try XCTUnwrap(fixture.scheduler.poll)
        timer.fire()
        let poll = try await fixture.client.next()
        XCTAssertEqual(poll.operation, .refresh(.local))
        XCTAssertNil(fixture.scheduler.poll)
        let count = fixture.client.operations.count
        fixture.sync.refresh()
        fixture.sync.refresh()
        fixture.sync.setWindowVisible(true)
        XCTAssertNil(fixture.scheduler.poll)
        XCTAssertEqual(fixture.client.operations.count, count)
        poll.succeed(emptyTaskListResources)
        try await fixture.settled()
        XCTAssertEqual(fixture.client.operations.count, count)
        XCTAssertEqual(fixture.scheduler.poll?.delay, 5)
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 1)
        fixture.scheduler.poll?.fire()
        let next = try await fixture.client.next()
        XCTAssertNil(fixture.scheduler.poll)
        next.succeed(emptyTaskListResources)
        try await fixture.settled()
        XCTAssertEqual(fixture.scheduler.poll?.delay, 5)
    }

    @MainActor
    func testConnectionTestSendsImmediatelyWhileAPollRuns() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.sync.refresh()
        let poll = try await fixture.client.next()
        let testing = Task { try await testConnectionSettings(serverSettings, client: fixture.client) }
        let test = try await fixture.client.next()
        XCTAssertEqual(test.operation, .test(serverSettings))
        test.tested()
        try await fixture.taskValue(testing)
        XCTAssertTrue(fixture.settings.writes.isEmpty)
        poll.succeed(emptyTaskListResources)
        try await fixture.settled()
    }

    @MainActor
    func testPollWithAChangedTaskNameNotifiesTheWindow() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: activeWorklog))
        let content = ObservationProbe { ViewReads.content(fixture.features) }
        let renamed = TaskItem(id: firstTask.id, name: "Renamed task", archived: false, latestStart: nil)

        fixture.scheduler.poll?.fire()
        let refresh = try await fixture.client.next()
        refresh.succeed(TaskListResources(tasks: [renamed], active: activeWorklog))
        try await fixture.settled()

        XCTAssertTrue(content.consume())
        XCTAssertEqual(fixture.features.taskCatalog.selectedTask?.name, "Renamed task")
    }
}
