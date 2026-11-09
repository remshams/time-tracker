import XCTest
@testable import TrackerClient

final class TrackerSyncSelectionAndReloadTests: XCTestCase {
    @MainActor
    func testTaskWriteReloadRunsThenAfterTheTaskListArrivesAndPublishesAroundIt() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: activeWorklog))
        // The closure type a task dialog store declares.
        let reload: @MainActor (_ then: () -> Void) async -> Void = fixture.sync.reloadAfterTaskWrite(then:)
        var tasksInThen: [TaskItem] = []
        var timerInThen: String?
        fixture.advanceClock(by: 4)

        let finished = Task { @MainActor in
            await reload {
                tasksInThen = fixture.session.tasks
                timerInThen = fixture.session.tracking.timerText
                fixture.advanceClock(by: 4)
            }
        }
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .refresh(fixture.session.connection.settings))
        XCTAssertEqual(
            fixture.session.tracking.timerText, "00:00:34", "The write result must show before the task list reloads.")
        fixture.advanceClock(by: 4)
        refresh.succeed(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        try await fixture.taskValue(finished)

        XCTAssertEqual(tasksInThen, [firstTask, secondTask])
        XCTAssertEqual(timerInThen, "00:00:38", "The reloaded task list must show before `then` runs.")
        XCTAssertEqual(fixture.session.tracking.timerText, "00:00:42", "What `then` changed must show afterwards.")
        XCTAssertFalse(fixture.client.hasPendingRequests, "A task write must not reload an unchanged history.")
    }

    @MainActor
    func testWorklogWriteReloadRefreshesTheTaskListThenTheHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let running = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(running)
        // The closure type a worklog dialog store declares.
        let reload: @MainActor () async -> Void = fixture.sync.reloadAfterWorklogWrite
        fixture.advanceClock(by: 4)

        let finished = Task { @MainActor in await reload() }
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .refresh(fixture.session.connection.settings))
        XCTAssertEqual(
            fixture.session.tracking.timerText, "00:00:34", "The write result must show before the task list reloads.")
        fixture.advanceClock(by: 4)
        refresh.succeed(running)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        try await fixture.taskValue(finished)
        XCTAssertEqual(fixture.session.tracking.timerText, "00:00:38", "The reloaded task list must show.")
        history.succeed(emptyPage)
        try await fixture.settled()
    }

    @MainActor
    func testChangingTheTabOrTaskLoadsTheHistoryAndPublishes() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: activeWorklog))

        fixture.advanceClock(by: 4)
        fixture.sync.selectTask(secondTask.id)
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, secondTask.id)
        XCTAssertEqual(fixture.session.tracking.timerText, "00:00:34")
        let selected = try await fixture.client.next()
        XCTAssertEqual(selected.operation, .history(task: secondTask.id, cursor: nil))
        selected.succeed(emptyPage)
        try await fixture.settled()

        fixture.advanceClock(by: 4)
        fixture.sync.changeTab(.archived)
        XCTAssertEqual(fixture.session.taskCatalog.tab, .archived)
        XCTAssertEqual(fixture.session.tracking.timerText, "00:00:38")
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
        let lists = ObservationProbe { ViewReads.lists(fixture.session) }
        let catalog = fixture.session.taskCatalog
        let worklogHistory = fixture.session.worklogHistory

        fixture.sync.selectTask(secondTask.id)
        let history = try await fixture.client.next()
        XCTAssertTrue(worklogHistory.isLoading)
        XCTAssertTrue(lists.consume())
        history.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "older", reset: false))
        try await fixture.settled()
        XCTAssertEqual(catalog.selectedTask, secondTask)
        XCTAssertEqual(worklogHistory.worklogs, [oldWorklog])
        XCTAssertTrue(worklogHistory.hasMore)
        XCTAssertTrue(lists.consume())

        fixture.sync.changeTab(.archived)
        XCTAssertEqual(catalog.tab, .archived)
        XCTAssertEqual(catalog.visibleTasks, [archivedTask])
        XCTAssertEqual(catalog.selectedTask, archivedTask)
        XCTAssertTrue(lists.consume())
        try await fixture.client.next().succeed(emptyPage)
        try await fixture.settled()
        fixture.sync.changeTab(.active)
        XCTAssertEqual(catalog.selectedTask, secondTask)
        XCTAssertTrue(lists.consume())
        try await fixture.client.next().succeed(emptyPage)
        try await fixture.settled()
    }

    @MainActor
    func testShowingATaskInTheOtherTabChangesTabAndSelectionAndLoadsItsHistoryOnce() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: nil))
        fixture.sync.changeTab(.archived)
        let archivedHistory = try await fixture.client.next()
        archivedHistory.succeed(emptyPage)
        try await fixture.settled()

        fixture.sync.showTask(secondTask)

        XCTAssertEqual(fixture.session.taskCatalog.tab, .active)
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, secondTask.id)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: secondTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.client.operations.filter { $0 == .history(task: secondTask.id, cursor: nil) }.count, 1)
        XCTAssertEqual(fixture.client.operations.last, .history(task: secondTask.id, cursor: nil))
    }

    @MainActor
    func testKeepingTheTabOrTaskSendsNothingAndDoesNotPublish() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        fixture.advanceClock(by: 4)

        fixture.sync.selectTask(firstTask.id)
        fixture.sync.changeTab(.active)

        XCTAssertEqual(fixture.session.tracking.timerText, "00:00:30", "Only a publish moves the timer text.")
        XCTAssertFalse(fixture.client.hasPendingRequests)
    }
}
