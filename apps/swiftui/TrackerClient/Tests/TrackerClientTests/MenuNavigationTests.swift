import XCTest
@testable import TrackerClient

final class MenuNavigationTests: XCTestCase {
    @MainActor
    func testRefreshKeepsSelectedTaskButRejectsActivationUntilItCompletes() async throws {
        let fixture = Fixture(saved: serverSettings, reports: true)
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask, secondTask], active: nil)
        let rows = [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 40_000_000),
                    TaskReportTotal(taskId: secondTask.id, durationMicroseconds: 10_000_000)]
        try await fixture.start(snapshot, rows: rows)
        let selected = TrackerMenuAction.task(id: secondTask.id)
        var navigation = TrackerMenuNavigation()
        let initial = TrackerMenuContent(fixture.session)
        navigation.update(actions: initial.availableActions(otherTasksExpanded: true),
                          displayedActions: initial.displayedActions(otherTasksExpanded: true))
        navigation.select(selected)
        fixture.scheduler.poll?.fire()
        let refresh = try await fixture.client.next()
        let busy = TrackerMenuContent(fixture.session)
        navigation.update(actions: busy.availableActions(otherTasksExpanded: true),
                          displayedActions: busy.displayedActions(otherTasksExpanded: true))
        XCTAssertEqual(navigation.selection, selected)
        XCTAssertFalse(navigation.actions.contains(selected))
        let operationCount = fixture.client.operations.count
        fixture.session.startTracking(taskID: secondTask.id)
        XCTAssertEqual(fixture.client.operations.count, operationCount)
        var moving = navigation
        moving.move(forward: true)
        XCTAssertEqual(moving.selection, .openTracker)
        moving = navigation
        moving.move(forward: false)
        XCTAssertEqual(moving.selection, .quit)
        refresh.succeed(TrackerReport(snapshot: snapshot, rows: rows))
        try await fixture.settled()
        let ready = TrackerMenuContent(fixture.session)
        navigation.update(actions: ready.availableActions(otherTasksExpanded: true),
                          displayedActions: ready.displayedActions(otherTasksExpanded: true))
        XCTAssertEqual(navigation.selection, selected)
        XCTAssertTrue(navigation.actions.contains(selected))
    }

    @MainActor
    func testKeyboardActionsSkipArchivedRowsAndHiddenTasksAndDisableWritesDuringRefresh() async throws {
        let fixture = Fixture(saved: serverSettings, reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask, secondTask, archivedTask], active: activeWorklog),
                                rows: [TaskReportTotal(taskId: archivedTask.id, durationMicroseconds: 30_000_000),
                                       TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 10_000_000)])
        let content = TrackerMenuContent(fixture.session)
        let collapsed = content.availableActions(otherTasksExpanded: false)
        XCTAssertFalse(collapsed.contains(.task(id: archivedTask.id)))
        XCTAssertFalse(collapsed.contains(.task(id: secondTask.id)))
        XCTAssertTrue(collapsed.contains(.stop(worklogID: activeWorklog.id)))
        let expanded = content.availableActions(otherTasksExpanded: true)
        XCTAssertTrue(expanded.contains(.task(id: secondTask.id)))
        fixture.scheduler.poll?.fire()
        _ = try await fixture.client.next()
        let refreshing = TrackerMenuContent(fixture.session).availableActions(otherTasksExpanded: true)
        XCTAssertFalse(refreshing.contains(.stop(worklogID: activeWorklog.id)))
        XCTAssertFalse(refreshing.contains(.task(id: secondTask.id)))
        XCTAssertFalse(refreshing.contains(.task(id: archivedTask.id)))
        var navigation = TrackerMenuNavigation()
        navigation.update(actions: refreshing)
        navigation.selectBoundary(first: false)
        navigation.move(forward: true)
        XCTAssertEqual(navigation.selection, .task(id: firstTask.id))
    }

    func testSelectionFollowsTaskIdentityWhenLiveTotalsReorderRows() {
        var navigation = TrackerMenuNavigation()
        let first = TrackerMenuAction.task(id: "first")
        let second = TrackerMenuAction.task(id: "second")
        navigation.update(actions: [first, second, .openTracker])
        navigation.select(second)
        navigation.update(actions: [second, first, .openTracker])
        XCTAssertEqual(navigation.selection, second)
        navigation.move(forward: true)
        XCTAssertEqual(navigation.selection, first)
    }

    func testUnavailableOrRemovedActionSelectsNearestRemainingAction() {
        var navigation = TrackerMenuNavigation()
        let stop = TrackerMenuAction.stop(worklogID: "old-worklog")
        let task = TrackerMenuAction.task(id: "task")
        navigation.update(actions: [stop, task, .openTracker, .quit])
        XCTAssertEqual(navigation.selection, stop)
        navigation.update(actions: [task, .openTracker, .quit])
        XCTAssertEqual(navigation.selection, task)
        navigation.update(actions: [.openTracker, .quit])
        XCTAssertEqual(navigation.selection, .openTracker)
        navigation.select(task)
        XCTAssertEqual(navigation.selection, .openTracker)
    }

    func testCollapsingOtherTasksKeepsDisclosureSelection() {
        var navigation = TrackerMenuNavigation()
        let task = TrackerMenuAction.task(id: "other-task")
        navigation.update(actions: [.otherTasks, task, .openTracker, .quit])
        navigation.select(task)
        navigation.select(.otherTasks)
        navigation.update(actions: [.otherTasks, .openTracker, .quit])
        XCTAssertEqual(navigation.selection, .otherTasks)
        navigation.move(forward: true)
        XCTAssertEqual(navigation.selection, .openTracker)
    }

    func testArrowNavigationWrapsAndBoundariesSelectFirstAndLast() {
        var navigation = TrackerMenuNavigation()
        navigation.update(actions: [.openTracker, .quit])
        navigation.move(forward: false)
        XCTAssertEqual(navigation.selection, .quit)
        navigation.move(forward: true)
        XCTAssertEqual(navigation.selection, .openTracker)
        navigation.selectBoundary(first: false)
        XCTAssertEqual(navigation.selection, .quit)
        navigation.selectBoundary(first: true)
        XCTAssertEqual(navigation.selection, .openTracker)
    }

    func testEmptyAndSingleActionListsRemainSafeDuringUpdates() {
        var navigation = TrackerMenuNavigation()
        navigation.move(forward: true)
        navigation.selectBoundary(first: false)
        XCTAssertNil(navigation.selection)
        navigation.update(actions: [.quit])
        navigation.move(forward: false)
        navigation.move(forward: true)
        XCTAssertEqual(navigation.selection, .quit)
        navigation.update(actions: [])
        XCTAssertNil(navigation.selection)
        navigation.select(.quit)
        XCTAssertNil(navigation.selection)
    }
}
