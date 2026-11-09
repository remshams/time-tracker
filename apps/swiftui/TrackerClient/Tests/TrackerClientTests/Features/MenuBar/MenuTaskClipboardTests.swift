import XCTest
@testable import TrackerClient

final class MenuTaskClipboardTests: XCTestCase {
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
        fixture.sync.selectTask(firstTask.id)
        XCTAssertEqual(
            fixture.features.menuBar.menuCopyValue(.copyName, taskID: secondTask.id, connection: .local), "Second task")
        XCTAssertEqual(
            fixture.features.menuBar.menuCopyValue(.copyExact, taskID: secondTask.id, connection: .local), "1h 23m 45s")
        XCTAssertEqual(
            fixture.features.menuBar.menuCopyValue(.copyRounded, taskID: secondTask.id, connection: .local), "1h 30m")
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
            fixture.features.menuBar.menuCopyValue(.copyExact, taskID: firstTask.id, connection: .local), "7m 35s")
        XCTAssertEqual(
            fixture.features.menuBar.menuCopyValue(.copyRounded, taskID: firstTask.id, connection: .local), "15m")
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
            fixture.features.menuBar.menuCopyValue(.copyExact, taskID: archivedTask.id, connection: .local), "20m 0s")
        XCTAssertEqual(
            fixture.features.menuBar.menuCopyValue(.copyName, taskID: archivedTask.id, connection: .local),
            archivedTask.name)
        XCTAssertEqual(fixture.client.operations.count, operations)
        XCTAssertNil(fixture.features.tracking.active)
    }

    @MainActor
    func testUnavailableTotalsDoNotBecomeZeroAndNameStillCopies() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        XCTAssertNil(fixture.features.menuBar.menuCopyValue(.copyExact, taskID: firstTask.id, connection: .local))
        XCTAssertNil(fixture.features.menuBar.menuCopyValue(.copyRounded, taskID: firstTask.id, connection: .local))
        XCTAssertEqual(
            fixture.features.menuBar.menuCopyValue(.copyName, taskID: firstTask.id, connection: .local), firstTask.name)
    }

    @MainActor
    func testLoadedTaskWithoutLogsCopiesZeroAndMissingTaskCopiesNothing() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil), rows: [])
        XCTAssertEqual(
            fixture.features.menuBar.menuCopyValue(.copyExact, taskID: firstTask.id, connection: .local), "0s")
        XCTAssertEqual(
            fixture.features.menuBar.menuCopyValue(.copyRounded, taskID: firstTask.id, connection: .local), "0m")
        XCTAssertNil(fixture.features.menuBar.menuCopyValue(.copyName, taskID: "missing-task", connection: .local))
        for action in [MenuShortcutAction.openMenu, .moveUp, .moveDown] {
            XCTAssertNil(fixture.features.menuBar.menuCopyValue(action, taskID: firstTask.id, connection: .local))
        }
    }

    @MainActor
    func testCopyRejectsDataSourceMismatchAndPreviousDayTotals() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(
            TaskListResources(tasks: [firstTask], active: nil),
            rows: [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 60_000_000)])
        for action in [MenuShortcutAction.copyName, .copyExact, .copyRounded] {
            XCTAssertNil(
                fixture.features.menuBar.menuCopyValue(action, taskID: firstTask.id, connection: serverSettings))
        }
        fixture.clock.now.addTimeInterval(86_400)
        fixture.clock.uptime += 86_400
        XCTAssertNil(fixture.features.menuBar.menuCopyValue(.copyExact, taskID: firstTask.id, connection: .local))
        XCTAssertNil(fixture.features.menuBar.menuCopyValue(.copyRounded, taskID: firstTask.id, connection: .local))
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
            fixture.features.menuBar.menuCopyValue(.copyExact, taskID: firstTask.id, connection: serverSettings),
            "1m 0s")
    }
}
