import XCTest
@testable import TrackerClient

final class MenuLiveValuesTests: XCTestCase {
    @MainActor
    func testSecondTicksUpdateExistingValuesWithoutRequestsOrNavigationChanges() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(
            TrackerSnapshot(tasks: [firstTask], active: activeWorklog),
            rows: [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 10_000_000)])
        let observer = TrackerMenuPresentationObserver(session: fixture.session, display: .none)
        observer.menuOpened(from: fixture.session, display: .none)
        fixture.session.menuOpened()
        let original = observer.content
        let requestCount = fixture.client.operations.count
        var valuesChanges = 0
        observer.onValuesChange = { valuesChanges += 1 }
        fixture.session.onChange = { observer.update(from: fixture.session, display: .none) }
        let timer = try XCTUnwrap(fixture.scheduler.display)
        fixture.clock.now.addTimeInterval(1)
        fixture.clock.uptime += 1
        timer.fire()
        XCTAssertEqual(observer.values.elapsedText, "00:00:31")
        XCTAssertEqual(observer.values.totalText, "00:00:11")
        XCTAssertEqual(observer.values.taskDurationTexts, [firstTask.id: "00:00:11"])
        XCTAssertEqual(observer.content, original)
        XCTAssertEqual(valuesChanges, 1)
        timer.fire()
        XCTAssertEqual(valuesChanges, 1, "An unchanged clock value must not redraw a field.")
        XCTAssertEqual(fixture.client.operations.count, requestCount)
    }

    @MainActor
    func testPollUpdatesOnlyCapturedTaskValuesAndRefreshesStructureAfterClosing() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(
            snapshot,
            rows: [
                TaskReportTotal(
                    taskId: firstTask.id,
                    durationMicroseconds: 10_000_000)
            ])
        let observer = TrackerMenuPresentationObserver(session: fixture.session, display: .time)
        observer.menuOpened(from: fixture.session, display: .time)
        let original = observer.content
        fixture.session.onChange = { observer.update(from: fixture.session, display: .time) }
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        poll.succeed(
            TrackerReport(
                snapshot: snapshot,
                rows: [
                    TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 20_000_000),
                    TaskReportTotal(taskId: secondTask.id, durationMicroseconds: 50_000_000),
                ]))
        let completed = Task { @MainActor in
            while fixture.session.isBusy && !Task.isCancelled { await Task.yield() }
        }
        try await fixture.taskValue(completed)
        XCTAssertEqual(observer.content, original)
        XCTAssertEqual(observer.values.totalText, "00:01:10")
        XCTAssertEqual(observer.values.taskDurationTexts, [firstTask.id: "00:00:20"])
        observer.menuClosed(from: fixture.session, display: .time)
        XCTAssertEqual(Set(observer.content.todayTasks.map(\.id)), [firstTask.id, secondTask.id])
        XCTAssertEqual(observer.values.taskDurationTexts[secondTask.id], "00:00:50")
    }

    @MainActor
    func testChangedOrStoppedWorklogCannotSupplyElapsedTimeToCapturedTask() async throws {
        for active in [
            WorklogItem?.none,
            WorklogItem(
                id: "replacement", taskId: firstTask.id,
                start: activeWorklog.start, end: nil),
        ] {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            try await fixture.start(TrackerSnapshot(tasks: [firstTask], active: activeWorklog))
            let observer = TrackerMenuPresentationObserver(session: fixture.session, display: .time)
            observer.menuOpened(from: fixture.session, display: .time)
            fixture.session.onChange = { observer.update(from: fixture.session, display: .time) }
            fixture.session.refresh()
            let poll = try await fixture.client.next()
            poll.succeed(TrackerSnapshot(tasks: [firstTask], active: active))
            let history = try await fixture.client.next()
            history.succeed(emptyPage)
            let completed = Task { @MainActor in
                while fixture.session.isBusy && !Task.isCancelled { await Task.yield() }
            }
            try await fixture.taskValue(completed)
            XCTAssertEqual(observer.content.activeWorklogID, activeWorklog.id)
            XCTAssertNil(observer.values.elapsedText)
            observer.menuClosed(from: fixture.session, display: .time)
            XCTAssertEqual(observer.content.activeWorklogID, active?.id)
            XCTAssertEqual(observer.values.elapsedText, active == nil ? nil : fixture.session.timerDisplayText)
        }
    }

    @MainActor
    func testConnectionChangeWithReusedIDsCannotUpdateCapturedMenuValues() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(
            snapshot,
            rows: [
                TaskReportTotal(
                    taskId: firstTask.id,
                    durationMicroseconds: 10_000_000)
            ])
        let observer = TrackerMenuPresentationObserver(session: fixture.session, display: .time)
        observer.menuOpened(from: fixture.session, display: .time)
        fixture.session.onChange = { observer.update(from: fixture.session, display: .time) }
        let connect = Task { await fixture.session.connect(serverSettings) }
        let request = try await fixture.client.next()
        request.succeed(snapshot)
        let connected = try await fixture.taskValue(connect)
        XCTAssertTrue(connected)
        let report = try await fixture.client.next()
        report.succeed(
            TrackerReport(
                snapshot: snapshot,
                rows: [
                    TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 900_000_000)
                ]))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        let completed = Task { @MainActor in
            while fixture.session.isBusy && !Task.isCancelled { await Task.yield() }
        }
        try await fixture.taskValue(completed)
        XCTAssertEqual(observer.content.connectionSettings, .local)
        XCTAssertNil(observer.values.elapsedText)
        XCTAssertEqual(observer.values.totalText, "Unavailable")
        XCTAssertEqual(observer.values.taskDurationTexts, [firstTask.id: "-"])
        XCTAssertEqual(
            observer.values.totalsExplanation,
            "The connection changed. Reopen the menu to see the current tracker.")
        observer.menuClosed(from: fixture.session, display: .time)
        XCTAssertEqual(observer.content.connectionSettings, serverSettings)
        XCTAssertEqual(observer.values.elapsedText, fixture.session.timerDisplayText)
    }

    @MainActor
    func testCachedValuesKeepTheirMarkerAndCurrentExplanationWhileMenuStaysOpen() async throws {
        let fixture = Fixture(saved: serverSettings, reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(
            TrackerSnapshot(tasks: [firstTask], active: activeWorklog),
            rows: [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 90_000_000)])
        let observer = TrackerMenuPresentationObserver(session: fixture.session, display: .time)
        observer.menuOpened(from: fixture.session, display: .time)
        fixture.session.onChange = { observer.update(from: fixture.session, display: .time) }
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        poll.fail(BridgeFailure(message: "Server unavailable", kind: "unavailable"))
        let completed = Task { @MainActor in
            while fixture.session.isBusy && !Task.isCancelled { await Task.yield() }
        }
        try await fixture.taskValue(completed)
        XCTAssertEqual(observer.content.dailyTotalsStatus, .current)
        XCTAssertEqual(observer.values.totalText, "~00:01:30")
        XCTAssertEqual(
            observer.values.totalsExplanation,
            "Today's total uses cached tracker state. Running time may be unconfirmed.")
        observer.menuClosed(from: fixture.session, display: .time)
        XCTAssertEqual(observer.values.totalText, "~00:01:30")
    }
}
