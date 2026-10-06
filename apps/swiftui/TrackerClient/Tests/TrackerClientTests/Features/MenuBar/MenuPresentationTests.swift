import XCTest
@testable import TrackerClient

final class MenuPresentationTests: XCTestCase {
    @MainActor
    func testOpenSubmenuKeepsNavigationStableWhileValuesAdvanceAcrossTicksAndPolls() async throws {
        let fixture = Fixture(saved: serverSettings, reports: true)
        defer { fixture.cleanup() }
        fixture.session.setWindowVisible(true)
        let snapshot = TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog)
        let rows = [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 10_000_000)]
        try await fixture.start(snapshot, rows: rows)
        let observer = TrackerMenuPresentationObserver(session: fixture.session, display: .time)
        var contentChanges = 0
        var labelChanges = 0
        var valueChanges = 0
        observer.onContentChange = { contentChanges += 1 }
        observer.onLabelChange = { labelChanges += 1 }
        observer.onValuesChange = { valueChanges += 1 }
        let window = TrackerPresentationObserver(session: fixture.session)
        var windowTicks = 0
        var windowControls = 0
        window.onTimerChange = { windowTicks += 1 }
        window.onActivityChange = { windowControls += 1 }
        fixture.session.onChange = {
            observer.update(from: fixture.session, display: .time)
            window.update(from: fixture.session)
        }
        observer.menuOpened(from: fixture.session, display: .time)
        observer.menuOpened(from: fixture.session, display: .time)
        let originalContent = observer.content
        let originalLabel = observer.label
        let display = try XCTUnwrap(fixture.scheduler.display)
        for step in 1...3 {
            fixture.clock.now.addTimeInterval(60)
            fixture.clock.uptime += 60
            display.fire()
            XCTAssertEqual(observer.values.elapsedText, fixture.session.timerDisplayText)
            XCTAssertEqual(observer.values.totalText, fixture.session.totalDailyDurationText)
            XCTAssertEqual(
                observer.values.taskDurationTexts[firstTask.id],
                fixture.session.dailyDurationText(taskID: firstTask.id))
            fixture.scheduler.poll?.fire()
            let poll = try await fixture.client.next()
            XCTAssertTrue(fixture.session.isBusy)
            XCTAssertEqual(observer.content, originalContent)
            let updatedRows = [
                TaskReportTotal(
                    taskId: firstTask.id,
                    durationMicroseconds: Int64(10 + step * 60) * 1_000_000)
            ]
            poll.succeed(TrackerReport(snapshot: snapshot, rows: updatedRows))
            let finished = Task { @MainActor in
                while fixture.session.isBusy && !Task.isCancelled { await Task.yield() }
            }
            try await fixture.taskValue(finished)
        }
        XCTAssertEqual(contentChanges, 0)
        XCTAssertEqual(labelChanges, 0)
        XCTAssertGreaterThanOrEqual(valueChanges, 3)
        XCTAssertEqual(windowTicks, 3, "Freezing a menu must not freeze the window's clock.")
        XCTAssertEqual(windowControls, 0, "Background polls must keep the window's controls stable.")
        XCTAssertEqual(observer.label, originalLabel)
        observer.menuClosed(from: fixture.session, display: .time)
        XCTAssertEqual(
            observer.content, originalContent, "Closing a submenu must not release the root menu's snapshot.")
        XCTAssertEqual(observer.values.elapsedText, fixture.session.timerDisplayText)
        observer.menuClosed(from: fixture.session, display: .time)
        XCTAssertEqual(contentChanges, 1)
        XCTAssertEqual(labelChanges, 1)
        XCTAssertEqual(observer.content.elapsedText, fixture.session.timerDisplayText)
        XCTAssertTrue(observer.content.canStopTracking)
    }

    @MainActor
    func testLabelPublishesOnlyMinuteChangesAndHonorsHiddenTotal() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(
            TrackerSnapshot(tasks: [firstTask], active: activeWorklog),
            rows: [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 10_000_000)])
        let observer = TrackerMenuPresentationObserver(session: fixture.session, display: .time)
        var changes = 0
        observer.onLabelChange = { changes += 1 }
        XCTAssertEqual(observer.label.symbol, "circle.fill")
        XCTAssertEqual(observer.label.status, "Tracking: First task")
        XCTAssertEqual(observer.label.text, "00:00")
        XCTAssertEqual(
            observer.label.help,
            "Tracking: First task\nTotal today: 00:00\nTime logged today in your local time zone")
        fixture.clock.now.addTimeInterval(1)
        fixture.clock.uptime += 1
        observer.update(from: fixture.session, display: .time)
        XCTAssertEqual(changes, 0, "Seconds in accessibility or help text must not invalidate the status label.")
        fixture.clock.now.addTimeInterval(49)
        fixture.clock.uptime += 49
        observer.update(from: fixture.session, display: .time)
        XCTAssertEqual(observer.label.text, "00:01")
        XCTAssertEqual(changes, 1)
        observer.update(from: fixture.session, display: .none)
        XCTAssertNil(observer.label.text)
        XCTAssertEqual(observer.label.help, observer.label.status)
        XCTAssertEqual(changes, 2)
        fixture.clock.now.addTimeInterval(3_600)
        fixture.clock.uptime += 3_600
        observer.update(from: fixture.session, display: .none)
        XCTAssertEqual(changes, 2)
        observer.update(from: fixture.session, display: .time)
        XCTAssertEqual(observer.label.text, "01:01")
        XCTAssertEqual(changes, 3)
    }

    @MainActor
    func testOpeningCapturesFreshTimeAndNestedNotificationsKeepTheSnapshotUntilFinalClose() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(
            TrackerSnapshot(tasks: [firstTask], active: activeWorklog),
            rows: [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 5_000_000)])
        let observer = TrackerMenuPresentationObserver(session: fixture.session, display: .time)
        fixture.clock.now.addTimeInterval(60)
        fixture.clock.uptime += 60
        observer.menuOpened(from: fixture.session, display: .time)
        XCTAssertEqual(observer.content.totalText, "00:01:05")
        XCTAssertEqual(observer.label.text, "00:01")
        fixture.clock.now.addTimeInterval(60)
        fixture.clock.uptime += 60
        observer.menuOpened(from: fixture.session, display: .none)
        XCTAssertEqual(observer.content.totalText, "00:01:05")
        XCTAssertEqual(observer.label.text, "00:01")
        observer.menuClosed(from: fixture.session, display: .none)
        XCTAssertEqual(observer.content.totalText, "00:01:05")
        XCTAssertEqual(observer.label.text, "00:01")
        observer.menuClosed(from: fixture.session, display: .none)
        XCTAssertEqual(observer.content.totalText, "00:02:05")
        XCTAssertNil(observer.label.text)
        observer.menuClosed(from: fixture.session, display: .time)
        XCTAssertEqual(observer.label.text, "00:02", "An unmatched close must not block later updates.")
        observer.update(from: fixture.session, display: .time)
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
            TrackerSnapshot(
                tasks: [secondTask, twin, archivedTask, emptyArchive, firstTask, another],
                active: activeWorklog), rows: rows)
        let observer = TrackerMenuPresentationObserver(session: fixture.session, display: .time)
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
        try await fixture.start(TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog))
        let observer = TrackerMenuPresentationObserver(session: fixture.session, display: .time)
        fixture.session.onChange = { observer.update(from: fixture.session, display: .time) }
        observer.menuOpened(from: fixture.session, display: .time)
        let target = try XCTUnwrap(observer.content.otherTasks.first { $0.id == secondTask.id })
        XCTAssertTrue(target.canStart)
        let changed = WorklogItem(
            id: "worklog-from-other-client", taskId: firstTask.id,
            start: activeWorklog.start, end: nil)
        let archived = TaskItem(id: secondTask.id, name: secondTask.name, archived: true, latestStart: nil)
        fixture.scheduler.poll?.fire()
        let poll = try await fixture.client.next()
        poll.succeed(TrackerSnapshot(tasks: [firstTask, archived], active: changed))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        let finished = Task { @MainActor in
            while fixture.session.isBusy && !Task.isCancelled { await Task.yield() }
        }
        try await fixture.taskValue(finished)
        let count = fixture.client.operations.count
        fixture.session.stopTracking(worklogID: try XCTUnwrap(observer.content.activeWorklogID))
        fixture.session.startTracking(taskID: target.id)
        XCTAssertEqual(fixture.client.operations.count, count)
        XCTAssertEqual(fixture.session.active?.id, changed.id)
        XCTAssertEqual(observer.content.activeWorklogID, activeWorklog.id)
        observer.menuClosed(from: fixture.session, display: .time)
        XCTAssertEqual(observer.content.activeWorklogID, changed.id)
        XCTAssertFalse(observer.content.otherTasks.contains { $0.id == secondTask.id })
    }

    @MainActor
    func testIdleAndUnavailableLabelsAndUnchangedContentDoNotNotify() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let observer = TrackerMenuPresentationObserver(session: fixture.session, display: .time)
        XCTAssertEqual(observer.label.symbol, "circle")
        XCTAssertEqual(observer.label.status, "Tracking status unavailable. No task tracked yet")
        XCTAssertEqual(observer.label.text, "-")
        XCTAssertEqual(observer.content.totalText, "Unavailable")
        XCTAssertEqual(observer.content.totalsExplanation, "Today's total is unavailable")
        XCTAssertNil(observer.content.activeWorklogID)
        XCTAssertNil(observer.content.elapsedText)
        XCTAssertTrue(observer.content.todayTasks.isEmpty)
        try await fixture.start(TrackerSnapshot(tasks: [firstTask], active: nil))
        observer.update(from: fixture.session, display: .time)
        XCTAssertEqual(observer.label.symbol, "circle")
        XCTAssertEqual(observer.label.status, "No task tracked yet")
        XCTAssertEqual(observer.content.otherTasks.first?.durationText, "-")
        var contentChanges = 0
        var labelChanges = 0
        observer.onContentChange = { contentChanges += 1 }
        observer.onLabelChange = { labelChanges += 1 }
        observer.update(from: fixture.session, display: .time)
        XCTAssertEqual(contentChanges, 0)
        XCTAssertEqual(labelChanges, 0)
    }

    @MainActor
    func testLoadingAndFailedReportsShareTheirExplanationWithTheMenu() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        fixture.session.start()
        let open = try await fixture.client.next()
        open.succeed(emptySnapshot)
        let report = try await fixture.client.next()
        let observer = TrackerMenuPresentationObserver(session: fixture.session, display: .time)
        XCTAssertEqual(observer.content.dailyTotalsStatus, .loading)
        XCTAssertEqual(observer.content.totalsExplanation, "Loading today's totals")
        report.fail(BridgeFailure(message: "Today's report failed"))
        try await fixture.settled()
        observer.update(from: fixture.session, display: .time)
        XCTAssertEqual(observer.content.dailyTotalsStatus, .unavailable)
        XCTAssertEqual(observer.content.totalsExplanation, "Today's report failed")
        XCTAssertEqual(observer.label.text, "-")
    }

    @MainActor
    func testReportFailureShowsCachedTotalAndStaleTrackingLabel() async throws {
        let fixture = Fixture(saved: serverSettings, reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(
            TrackerSnapshot(tasks: [firstTask], active: activeWorklog),
            rows: [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 90_000_000)])
        let observer = TrackerMenuPresentationObserver(session: fixture.session, display: .time)
        fixture.scheduler.poll?.fire()
        let poll = try await fixture.client.next()
        poll.fail(BridgeFailure(message: "Server unavailable", kind: "unavailable"))
        try await fixture.settled()
        observer.update(from: fixture.session, display: .time)
        XCTAssertTrue(observer.content.isStale)
        XCTAssertEqual(observer.label.symbol, "circle")
        XCTAssertEqual(observer.label.status, "Tracking status unavailable. Last confirmed task: First task")
        XCTAssertEqual(observer.label.text, "~00:01")
        XCTAssertEqual(observer.content.dailyTotalsStatus, .cached)
        XCTAssertEqual(
            observer.content.totalsExplanation,
            "Today's total uses cached tracker state. Running time may be unconfirmed.")
        XCTAssertFalse(observer.content.canStopTracking)
    }
}
