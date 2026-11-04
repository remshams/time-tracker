import XCTest
@testable import TrackerClient

final class MenuPresentationTests: XCTestCase {
    @MainActor
    func testOpenSubmenuKeepsNavigationStableWhileValuesAdvanceAcrossTicksAndPolls() async throws {
        let fixture = Fixture(saved: serverSettings, reports: true)
        defer { fixture.cleanup() }
        fixture.session.setWindowVisible(true)
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
        let rows = [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 10_000_000)]
        try await fixture.start(resources, rows: rows)
        let menu = MenuHarness(fixture, showDailyTotal: true)
        let observer = menu.observer
        var labelChanges = 0
        var valueChanges = 0
        observer.onLabelChange = { labelChanges += 1 }
        observer.onValuesChange = { valueChanges += 1 }
        let window = PresentationHarness(session: fixture.session)
        let windowTicks = window.counter(PresentationReads.timer)
        let windowControls = window.counter(PresentationReads.controls)
        fixture.session.onChange = {
            menu.update()
            window.update()
        }
        observer.menuOpened()
        let originalContent = observer.content
        let originalLabel = observer.label
        let display = try XCTUnwrap(fixture.scheduler.display)
        for step in 1...3 {
            fixture.clock.now.addTimeInterval(60)
            fixture.clock.uptime += 60
            display.fire()
            try await menu.settled()
            let totals = fixture.session.dailyTotals
            XCTAssertEqual(observer.values.elapsedText, fixture.session.tracking.timerText)
            XCTAssertEqual(observer.values.totalText, totals.dayTotal.map(formatClockDuration))
            XCTAssertEqual(
                observer.values.taskDurationTexts[firstTask.id],
                totals.taskDurations[firstTask.id].map(formatClockDuration))
            fixture.scheduler.poll?.fire()
            let poll = try await fixture.client.next()
            XCTAssertTrue(fixture.client.hasPendingRequests)
            XCTAssertEqual(observer.content, originalContent)
            let updatedRows = [
                TaskReportTotal(
                    taskId: firstTask.id,
                    durationMicroseconds: Int64(10 + step * 60) * 1_000_000)
            ]
            poll.succeed(TaskListTotalsRefresh(taskList: resources, rows: updatedRows))
            let finished = Task { @MainActor in
                while fixture.client.hasPendingRequests && !Task.isCancelled { await Task.yield() }
            }
            try await fixture.taskValue(finished)
            try await menu.settled()
        }
        XCTAssertEqual(observer.content, originalContent)
        XCTAssertEqual(labelChanges, 0)
        XCTAssertGreaterThanOrEqual(valueChanges, 3)
        XCTAssertEqual(windowTicks.count, 3, "Freezing a menu must not freeze the window's clock.")
        XCTAssertEqual(windowControls.count, 0, "Background polls must keep the window's controls stable.")
        XCTAssertEqual(observer.label, originalLabel)
        observer.menuClosed()
        XCTAssertNotEqual(observer.content, originalContent)
        XCTAssertEqual(labelChanges, 1)
        XCTAssertEqual(observer.content.elapsedText, fixture.session.tracking.timerText)
        XCTAssertTrue(observer.content.canStopTracking)
    }

    @MainActor
    func testClosedMenuKeepsObservingEveryStoreAfterChangesWhileOpen() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        let menu = MenuHarness(fixture, showDailyTotal: false)
        let observer = menu.observer
        var labelChanges = 0
        observer.onLabelChange = { labelChanges += 1 }
        observer.menuOpened()
        fixture.scheduler.poll?.fire()
        let stop = try await fixture.client.next()
        stop.succeed(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        try await menu.settled()
        XCTAssertEqual(observer.label.status, "Tracking: First task")
        XCTAssertEqual(labelChanges, 0)
        observer.menuClosed()
        XCTAssertEqual(observer.label.status, "Stopped. Last tracked: First task")
        XCTAssertEqual(labelChanges, 1)
        let replacement = WorklogItem(id: "replacement", taskId: secondTask.id, start: activeWorklog.start, end: nil)
        fixture.scheduler.poll?.fire()
        let start = try await fixture.client.next()
        start.succeed(TaskListResources(tasks: [firstTask, secondTask], active: replacement))
        try await fixture.settled()
        try await menu.settled()
        XCTAssertEqual(observer.label.status, "Tracking: Second task")
        XCTAssertEqual(labelChanges, 2)
    }

    @MainActor
    func testLabelPublishesOnlyMinuteChangesAndHonorsHiddenTotal() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(
            TaskListResources(tasks: [firstTask], active: activeWorklog),
            rows: [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 10_000_000)])
        fixture.session.setMenuBarClock(true)
        let menu = MenuHarness(fixture, showDailyTotal: true)
        let observer = menu.observer
        var changes = 0
        observer.onLabelChange = { changes += 1 }
        XCTAssertEqual(observer.label.indicator.symbol, "circle.fill")
        XCTAssertEqual(observer.label.status, "Tracking: First task")
        XCTAssertEqual(observer.label.totalText, "00:00")
        XCTAssertEqual(
            observer.label.help,
            "Tracking: First task\nTotal today: 00:00\nTime logged today in your local time zone")
        try await menu.tick(1)
        XCTAssertEqual(changes, 0, "Seconds in accessibility or help text must not invalidate the status label.")
        try await menu.tick(49)
        XCTAssertEqual(observer.label.totalText, "00:01")
        XCTAssertEqual(changes, 1)
        observer.setShowDailyTotal(false)
        XCTAssertNil(observer.label.totalText)
        XCTAssertEqual(observer.label.help, observer.label.status)
        XCTAssertEqual(changes, 2)
        try await menu.tick(3_600)
        XCTAssertEqual(changes, 2)
        observer.setShowDailyTotal(true)
        XCTAssertEqual(observer.label.totalText, "01:01")
        XCTAssertEqual(changes, 3)
    }

    @MainActor
    func testOpeningCapturesFreshTimeAndUpdatesKeepTheSnapshotUntilClose() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(
            TaskListResources(tasks: [firstTask], active: activeWorklog),
            rows: [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 5_000_000)])
        let menu = MenuHarness(fixture, showDailyTotal: true)
        let observer = menu.observer
        fixture.clock.now.addTimeInterval(60)
        fixture.clock.uptime += 60
        menu.menuOpened()
        XCTAssertEqual(observer.content.totalText, "00:01:05")
        XCTAssertEqual(observer.label.totalText, "00:01")
        fixture.clock.now.addTimeInterval(60)
        fixture.clock.uptime += 60
        observer.setShowDailyTotal(false)
        XCTAssertEqual(observer.content.totalText, "00:01:05")
        XCTAssertEqual(observer.label.totalText, "00:01")
        menu.menuClosed()
        XCTAssertEqual(observer.content.totalText, "00:02:05")
        XCTAssertNil(observer.label.totalText)
        observer.setShowDailyTotal(true)
        XCTAssertEqual(observer.label.totalText, "00:02")
        XCTAssertEqual(observer.content.totalText, "00:02:05")
    }

    @MainActor
    func testMenuEntriesUseDailyTotalsAndSortTasksWithoutTimeByNameThenID() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        let twin = TaskItem(id: "task-three", name: secondTask.name, archived: false, latestStart: nil)
        let another = TaskItem(id: "task-zulu", name: "Another task", archived: false, latestStart: nil)
        let emptyArchive = TaskItem(id: "archive-empty", name: "Empty archived task", archived: true, latestStart: nil)
        let rows = [
            TaskReportTotal(taskId: archivedTask.id, durationMicroseconds: 30_000_000),
            TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 10_000_000),
        ]
        try await fixture.start(
            TaskListResources(
                tasks: [secondTask, twin, archivedTask, emptyArchive, firstTask, another],
                active: activeWorklog), rows: rows)
        let observer = MenuHarness(fixture, showDailyTotal: true).observer
        XCTAssertEqual(observer.content.todayTasks.map(\.id), [archivedTask.id, firstTask.id])
        XCTAssertEqual(observer.content.todayTasks.map(\.durationText), ["00:00:30", "00:00:10"])
        XCTAssertEqual(observer.content.todayTasks.map(\.isRunning), [false, true])
        XCTAssertEqual(observer.content.todayTasks.map(\.canStart), [false, false])
        XCTAssertEqual(observer.content.otherTasks.map(\.id), [another.id, twin.id, secondTask.id])
        XCTAssertTrue(observer.content.otherTasks.allSatisfy(\.canStart))
        XCTAssertEqual(observer.content.totalText, "00:00:40")
        XCTAssertEqual(observer.content.activeWorklogID, activeWorklog.id)
        XCTAssertTrue(observer.content.canStopTracking)
    }

    @MainActor
    func testFrozenMenuTargetsAreRevalidatedAgainstChangedServerState() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        let menu = MenuHarness(fixture, showDailyTotal: true)
        let observer = menu.observer
        observer.menuOpened()
        let target = try XCTUnwrap(observer.content.otherTasks.first { $0.id == secondTask.id })
        XCTAssertTrue(target.canStart)
        let changed = WorklogItem(
            id: "worklog-from-other-client", taskId: firstTask.id,
            start: activeWorklog.start, end: nil)
        let archived = TaskItem(id: secondTask.id, name: secondTask.name, archived: true, latestStart: nil)
        fixture.scheduler.poll?.fire()
        let poll = try await fixture.client.next()
        poll.succeed(TaskListResources(tasks: [firstTask, archived], active: changed))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        let finished = Task { @MainActor in
            while fixture.client.hasPendingRequests && !Task.isCancelled { await Task.yield() }
        }
        try await fixture.taskValue(finished)
        try await menu.settled()
        let count = fixture.client.operations.count
        fixture.session.stopTracking(worklogID: try XCTUnwrap(observer.content.activeWorklogID))
        fixture.session.startTracking(taskID: target.id)
        XCTAssertEqual(fixture.client.operations.count, count)
        XCTAssertEqual(fixture.session.active?.id, changed.id)
        XCTAssertEqual(observer.content.activeWorklogID, activeWorklog.id)
        observer.menuClosed()
        XCTAssertEqual(observer.content.activeWorklogID, changed.id)
        XCTAssertFalse(observer.content.otherTasks.contains { $0.id == secondTask.id })
    }

    @MainActor
    func testIdleAndUnavailableLabelsAndUnchangedContentDoNotNotify() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let menu = MenuHarness(fixture, showDailyTotal: true)
        let observer = menu.observer
        XCTAssertEqual(observer.label.indicator.symbol, "circle")
        XCTAssertEqual(observer.label.status, "Tracking status unavailable. No task tracked yet")
        XCTAssertEqual(observer.label.totalText, "-")
        XCTAssertEqual(observer.content.totalText, "Unavailable")
        XCTAssertEqual(observer.content.totalsExplanation, "Today's total is unavailable")
        XCTAssertNil(observer.content.activeWorklogID)
        XCTAssertNil(observer.content.elapsedText)
        XCTAssertTrue(observer.content.todayTasks.isEmpty)
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        try await menu.settled()
        XCTAssertEqual(observer.label.indicator.symbol, "circle")
        XCTAssertEqual(observer.label.status, "No task tracked yet")
        XCTAssertEqual(observer.content.otherTasks.first?.durationText, "-")
        let content = observer.content
        var labelChanges = 0
        var valueChanges = 0
        observer.onLabelChange = { labelChanges += 1 }
        observer.onValuesChange = { valueChanges += 1 }
        observer.setShowDailyTotal(true)
        XCTAssertEqual(observer.content, content)
        XCTAssertEqual(labelChanges, 0)
        XCTAssertEqual(valueChanges, 0)
    }

    @MainActor
    func testLoadingAndFailedReportsShareTheirExplanationWithTheMenu() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        fixture.session.start()
        let open = try await fixture.client.next()
        open.succeed(emptyTaskListResources)
        let report = try await fixture.client.next()
        let menu = MenuHarness(fixture, showDailyTotal: true)
        let observer = menu.observer
        XCTAssertEqual(observer.content.dailyTotalsStatus, .loading)
        XCTAssertEqual(observer.content.totalsExplanation, "Loading today's totals")
        report.fail(BridgeFailure(message: "Today's report failed"))
        try await fixture.settled()
        try await menu.settled()
        XCTAssertEqual(observer.content.dailyTotalsStatus, .unavailable)
        XCTAssertEqual(observer.content.totalsExplanation, "Today's report failed")
        XCTAssertEqual(observer.label.totalText, "-")
    }

    @MainActor
    func testReportFailureShowsCachedTotalAndStaleTrackingLabel() async throws {
        let fixture = Fixture(saved: serverSettings, reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(
            TaskListResources(tasks: [firstTask], active: activeWorklog),
            rows: [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 90_000_000)])
        let menu = MenuHarness(fixture, showDailyTotal: true)
        let observer = menu.observer
        fixture.scheduler.poll?.fire()
        let poll = try await fixture.client.next()
        poll.fail(BridgeFailure(message: "Server unavailable", kind: "unavailable"))
        try await fixture.settled()
        try await menu.settled()
        XCTAssertTrue(observer.content.isStale)
        XCTAssertEqual(observer.label.indicator.symbol, "circle")
        XCTAssertEqual(observer.label.status, "Tracking status unavailable. Last confirmed task: First task")
        XCTAssertEqual(observer.label.totalText, "~00:01")
        XCTAssertEqual(observer.content.dailyTotalsStatus, .cached)
        XCTAssertEqual(
            observer.content.totalsExplanation,
            "Today's total uses cached tracker state. Running time may be unconfirmed.")
        XCTAssertFalse(observer.content.canStopTracking)
    }
}
