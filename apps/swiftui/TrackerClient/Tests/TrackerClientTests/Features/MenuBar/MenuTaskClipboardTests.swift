import XCTest
@testable import TrackerClient

final class MenuTaskClipboardTests: XCTestCase {
    @MainActor
    func testCopyUsesHighlightedTaskRatherThanWindowSelectionOrRunningTask() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog)
        let rows = [
            TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 30_000_000),
            TaskReportTotal(taskId: secondTask.id, durationMicroseconds: 5_025_000_000),
        ]
        try await fixture.start(snapshot, rows: rows)
        fixture.session.select(firstTask.id)
        XCTAssertEqual(
            fixture.session.menuCopyValue(.copyName, taskID: secondTask.id, connection: .local), "Second task")
        XCTAssertEqual(
            fixture.session.menuCopyValue(.copyExact, taskID: secondTask.id, connection: .local), "1h 23m 45s")
        XCTAssertEqual(fixture.session.menuCopyValue(.copyRounded, taskID: secondTask.id, connection: .local), "1h 30m")
        XCTAssertEqual(fixture.session.selectedTaskID, firstTask.id)
        XCTAssertEqual(fixture.session.active, activeWorklog)
    }

    @MainActor
    func testCopyProjectsRunningTimeAtKeyPressWhilePresentationRemainsStable() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(
            TrackerSnapshot(tasks: [firstTask], active: activeWorklog),
            rows: [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 440_000_000)])
        let observer = TrackerMenuPresentationObserver(session: fixture.session, showDailyTotal: true)
        observer.menuOpened(from: fixture.session, showDailyTotal: true)
        let captured = observer.content
        fixture.clock.now.addTimeInterval(15)
        fixture.clock.uptime += 15
        observer.update(from: fixture.session, showDailyTotal: true)
        XCTAssertEqual(observer.content, captured)
        XCTAssertEqual(fixture.session.menuCopyValue(.copyExact, taskID: firstTask.id, connection: .local), "7m 35s")
        XCTAssertEqual(fixture.session.menuCopyValue(.copyRounded, taskID: firstTask.id, connection: .local), "15m")
        observer.menuClosed(from: fixture.session, showDailyTotal: true)
    }

    @MainActor
    func testArchivedTaskCanBeCopiedWithoutStartingTracking() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(
            TrackerSnapshot(tasks: [archivedTask], active: nil),
            rows: [TaskReportTotal(taskId: archivedTask.id, durationMicroseconds: 1_200_000_000)])
        let operations = fixture.client.operations.count
        XCTAssertFalse(fixture.session.canStartTracking(taskID: archivedTask.id))
        XCTAssertEqual(fixture.session.menuCopyValue(.copyExact, taskID: archivedTask.id, connection: .local), "20m 0s")
        XCTAssertEqual(
            fixture.session.menuCopyValue(.copyName, taskID: archivedTask.id, connection: .local), archivedTask.name)
        XCTAssertEqual(fixture.client.operations.count, operations)
        XCTAssertNil(fixture.session.active)
    }

    @MainActor
    func testUnavailableTotalsDoNotBecomeZeroAndNameStillCopies() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask], active: nil))
        XCTAssertNil(fixture.session.menuCopyValue(.copyExact, taskID: firstTask.id, connection: .local))
        XCTAssertNil(fixture.session.menuCopyValue(.copyRounded, taskID: firstTask.id, connection: .local))
        XCTAssertEqual(
            fixture.session.menuCopyValue(.copyName, taskID: firstTask.id, connection: .local), firstTask.name)
    }

    @MainActor
    func testLoadedTaskWithoutLogsCopiesZeroAndMissingTaskCopiesNothing() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask], active: nil), rows: [])
        XCTAssertEqual(fixture.session.menuCopyValue(.copyExact, taskID: firstTask.id, connection: .local), "0s")
        XCTAssertEqual(fixture.session.menuCopyValue(.copyRounded, taskID: firstTask.id, connection: .local), "0m")
        XCTAssertNil(fixture.session.menuCopyValue(.copyName, taskID: "missing-task", connection: .local))
        for action in [MenuShortcutAction.openMenu, .moveUp, .moveDown] {
            XCTAssertNil(fixture.session.menuCopyValue(action, taskID: firstTask.id, connection: .local))
        }
    }

    @MainActor
    func testCopyRejectsDataSourceMismatchAndPreviousDayTotals() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(
            TrackerSnapshot(tasks: [firstTask], active: nil),
            rows: [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 60_000_000)])
        for action in [MenuShortcutAction.copyName, .copyExact, .copyRounded] {
            XCTAssertNil(fixture.session.menuCopyValue(action, taskID: firstTask.id, connection: serverSettings))
        }
        fixture.clock.now.addTimeInterval(86_400)
        fixture.clock.uptime += 86_400
        XCTAssertNil(fixture.session.menuCopyValue(.copyExact, taskID: firstTask.id, connection: .local))
        XCTAssertNil(fixture.session.menuCopyValue(.copyRounded, taskID: firstTask.id, connection: .local))
    }

    @MainActor
    func testCachedTotalsRemainCopyableWithTheSessionMarkingThemCached() async throws {
        let fixture = Fixture(saved: serverSettings, reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(
            TrackerSnapshot(tasks: [firstTask], active: nil),
            rows: [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 60_000_000)])
        fixture.scheduler.poll?.fire()
        let poll = try await fixture.client.next()
        poll.fail(BridgeFailure(message: "Server unavailable", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.dailyTotalsStatus, .cached)
        XCTAssertEqual(
            fixture.session.menuCopyValue(.copyExact, taskID: firstTask.id, connection: serverSettings), "1m 0s")
    }
}
