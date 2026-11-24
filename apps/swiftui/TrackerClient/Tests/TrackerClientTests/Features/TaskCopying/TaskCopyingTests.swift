import XCTest
@testable import TrackerClient

final class TaskCopyingTests: XCTestCase {
    @MainActor
    func testCopyUsesHighlightedTaskRatherThanWindowSelectionOrRunningTask() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
        let rows = [
            TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 30_000_000),
            TaskReportTotal(taskId: secondTask.id, durationMicroseconds: 5_025_000_000),
        ]
        try await fixture.start(resources, rows: rows)
        fixture.features.taskCatalog.select(firstTask.id)
        XCTAssertEqual(
            fixture.features.taskCopying.value(.copyName, taskID: secondTask.id), "Second task")
        XCTAssertEqual(
            fixture.features.taskCopying.value(.copyExact, taskID: secondTask.id), "1h 23m 45s")
        XCTAssertEqual(
            fixture.features.taskCopying.value(.copyRounded, taskID: secondTask.id), "1h 30m")
        XCTAssertEqual(fixture.features.taskCatalog.selectedTaskID, firstTask.id)
        XCTAssertEqual(fixture.features.tracking.active, activeWorklog)
    }

    @MainActor
    func testCopyProjectsRunningTimeAtKeyPressWhilePresentationRemainsStable() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(
            TaskListResources(tasks: [firstTask], active: activeWorklog),
            rows: [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 440_000_000)])
        let menu = MenuHarness(fixture, showDailyTotal: true)
        let observer = menu.observer
        observer.menuOpened()
        let captured = observer.content
        fixture.clock.now.addTimeInterval(15)
        fixture.clock.uptime += 15
        try await menu.settled()
        XCTAssertEqual(observer.content, captured)
        XCTAssertEqual(
            fixture.features.taskCopying.value(.copyExact, taskID: firstTask.id), "7m 35s")
        XCTAssertEqual(
            fixture.features.taskCopying.value(.copyRounded, taskID: firstTask.id), "15m")
        observer.menuClosed()
    }

    @MainActor
    func testArchivedTaskCanBeCopiedWithoutStartingTracking() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(
            TaskListResources(tasks: [archivedTask], active: nil),
            rows: [TaskReportTotal(taskId: archivedTask.id, durationMicroseconds: 1_200_000_000)])
        let operations = fixture.client.operations.count
        XCTAssertFalse(fixture.trackingAvailability.canStart(taskID: archivedTask.id))
        XCTAssertEqual(
            fixture.features.taskCopying.value(.copyExact, taskID: archivedTask.id), "20m 0s")
        XCTAssertEqual(
            fixture.features.taskCopying.value(.copyName, taskID: archivedTask.id),
            archivedTask.name)
        XCTAssertEqual(fixture.client.operations.count, operations)
        XCTAssertNil(fixture.features.tracking.active)
    }

    @MainActor
    func testUnavailableTotalsDoNotBecomeZeroAndNameStillCopies() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        XCTAssertNil(fixture.features.taskCopying.value(.copyExact, taskID: firstTask.id))
        XCTAssertNil(fixture.features.taskCopying.value(.copyRounded, taskID: firstTask.id))
        XCTAssertEqual(
            fixture.features.taskCopying.value(.copyName, taskID: firstTask.id), firstTask.name)
    }

    @MainActor
    func testLoadedTaskWithoutLogsCopiesZeroAndMissingTaskCopiesNothing() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil), rows: [])
        XCTAssertEqual(
            fixture.features.taskCopying.value(.copyExact, taskID: firstTask.id), "0s")
        XCTAssertEqual(
            fixture.features.taskCopying.value(.copyRounded, taskID: firstTask.id), "0m")
        XCTAssertNil(fixture.features.taskCopying.value(.copyName, taskID: "missing-task"))
    }

    @MainActor
    func testCopyRejectsPreviousDayTotals() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(
            TaskListResources(tasks: [firstTask], active: nil),
            rows: [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 60_000_000)])
        fixture.clock.now.addTimeInterval(86_400)
        fixture.clock.uptime += 86_400
        XCTAssertNil(fixture.features.taskCopying.value(.copyExact, taskID: firstTask.id))
        XCTAssertNil(fixture.features.taskCopying.value(.copyRounded, taskID: firstTask.id))
    }

    @MainActor
    func testCachedTotalsRemainCopyableMarkedAsCached() async throws {
        let fixture = Fixture(saved: serverSettings, reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(
            TaskListResources(tasks: [firstTask], active: nil),
            rows: [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 60_000_000)])
        fixture.scheduler.poll?.fire()
        let poll = try await fixture.client.next()
        poll.fail(BridgeFailure(message: "Server unavailable", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertEqual(fixture.features.dailyTotals.status, .cached)
        XCTAssertEqual(
            fixture.features.taskCopying.value(.copyExact, taskID: firstTask.id),
            "1m 0s")
    }
}
