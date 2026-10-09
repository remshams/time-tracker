import Foundation
import XCTest
@testable import TrackerClient

private func dailyCalendar(_ zone: String = "UTC") -> Calendar {
    var calendar = Calendar(identifier: .gregorian)
    calendar.timeZone = TimeZone(identifier: zone)!
    return calendar
}

private func dailyDate(_ value: String) -> Date { timestamp(value)! }

private func total(_ task: TaskItem, _ seconds: Int64) -> TaskReportTotal {
    TaskReportTotal(taskId: task.id, durationMicroseconds: seconds * 1_000_000)
}

final class DailyTotalsTests: XCTestCase {
    @MainActor
    func testAggregateHandlesMicrosecondSumAboveInt64WithoutOverflow() async throws {
        let clock = FakeClock()
        let state = DailyTotalsState(calendar: dailyCalendar())
        let request = try XCTUnwrap(state.begin(clock: clock))
        let rows = [firstTask, secondTask].map {
            TaskReportTotal(taskId: $0.id, durationMicroseconds: Int64.max)
        }
        state.accept(
            TrackerReport(snapshot: TrackerSnapshot(tasks: [firstTask, secondTask], active: nil), rows: rows),
            requested: request, clock: clock)
        XCTAssertEqual(state.totalDuration(active: nil, clock: clock), 18_446_744_073_709.55)
    }

    @MainActor
    func testAggregateDistinguishesUnavailableDataFromLoadedEmptyAndCachedEmptyReports() async throws {
        let clock = FakeClock()
        let state = DailyTotalsState(calendar: dailyCalendar())
        XCTAssertNil(state.totalDuration(active: nil, clock: clock))
        let request = try XCTUnwrap(state.begin(clock: clock))
        XCTAssertNil(state.totalDuration(active: nil, clock: clock))
        state.accept(TrackerReport(snapshot: emptySnapshot, rows: []), requested: request, clock: clock)
        XCTAssertEqual(state.totalDuration(active: nil, clock: clock), 0)
        state.fail(BridgeFailure(message: "Report unavailable"), clock: clock)
        XCTAssertEqual(state.status, .cached)
        XCTAssertEqual(state.totalDuration(active: nil, clock: clock), 0)
        clock.now.addTimeInterval(86_400)
        XCTAssertNil(state.totalDuration(active: nil, clock: clock))
    }

    @MainActor
    func testAggregateIncludesArchivedRowsAndProjectsTheMatchingRunningWorklogOnce() async throws {
        let clock = FakeClock()
        let state = DailyTotalsState(calendar: dailyCalendar())
        let request = try XCTUnwrap(state.begin(clock: clock))
        state.accept(
            TrackerReport(
                snapshot: TrackerSnapshot(tasks: [firstTask, secondTask, archivedTask], active: activeWorklog),
                rows: [total(firstTask, 20), total(secondTask, 5), total(archivedTask, 50)]),
            requested: request, clock: clock)
        XCTAssertEqual(state.totalDuration(active: activeWorklog, clock: clock), 75)
        clock.now.addTimeInterval(7)
        clock.uptime += 7
        XCTAssertEqual(state.totalDuration(active: activeWorklog, clock: clock), 82)
        XCTAssertEqual(state.totalDuration(active: nil, clock: clock), 75)
        let changed = WorklogItem(id: "changed-worklog", taskId: firstTask.id, start: activeWorklog.start, end: nil)
        XCTAssertEqual(state.totalDuration(active: changed, clock: clock), 75)
        state.fail(BridgeFailure(message: "Report unavailable"), clock: clock)
        XCTAssertEqual(state.totalDuration(active: activeWorklog, clock: clock), 82)
        state.clear(at: clock.now)
        XCTAssertNil(state.totalDuration(active: activeWorklog, clock: clock))
    }

    @MainActor
    func testAggregateProjectsRunningTaskWhenItsReportRowIsAbsent() async throws {
        let clock = FakeClock()
        let state = DailyTotalsState(calendar: dailyCalendar())
        let request = try XCTUnwrap(state.begin(clock: clock))
        state.accept(
            TrackerReport(
                snapshot: TrackerSnapshot(tasks: [firstTask, archivedTask], active: activeWorklog),
                rows: [total(archivedTask, 50)]), requested: request, clock: clock)
        clock.uptime += 3
        clock.now.addTimeInterval(3)
        XCTAssertEqual(state.totalDuration(active: activeWorklog, clock: clock), 53)
    }

    @MainActor
    func testReportIncludesRunningTimeOnceAndAddsOnlyTimeAfterCapturedCutoff() async throws {
        let clock = FakeClock()
        clock.now = dailyDate("2025-01-01T12:00:00.123Z")
        let state = DailyTotalsState(calendar: dailyCalendar())
        let request = try XCTUnwrap(state.begin(clock: clock))
        XCTAssertEqual(request.now, "2025-01-01T12:00:00.123Z")
        clock.uptime += 7
        clock.now.addTimeInterval(7)
        state.accept(
            TrackerReport(
                snapshot: TrackerSnapshot(tasks: [firstTask], active: activeWorklog),
                rows: [total(firstTask, 43_200)]), requested: request, clock: clock)
        XCTAssertEqual(state.duration(taskID: firstTask.id, active: activeWorklog, clock: clock), 43_207)
        XCTAssertEqual(state.duration(taskID: secondTask.id, active: activeWorklog, clock: clock), 0)
    }

    @MainActor
    func testLocalDayIntervalsUseTwentyThreeAndTwentyFiveHoursAcrossDaylightSaving() async throws {
        let clock = FakeClock()
        let state = DailyTotalsState(calendar: dailyCalendar("Europe/Berlin"))
        clock.now = dailyDate("2025-03-30T12:00:00.000Z")
        let spring = try XCTUnwrap(state.begin(clock: clock))
        XCTAssertEqual(spring.start, "2025-03-29T23:00:00.000Z")
        XCTAssertEqual(spring.end, "2025-03-30T22:00:00.000Z")
        XCTAssertEqual(spring.day.duration, 23 * 3_600)
        clock.now = dailyDate("2025-10-26T12:00:00.000Z")
        let autumn = try XCTUnwrap(state.begin(clock: clock))
        XCTAssertEqual(autumn.start, "2025-10-25T22:00:00.000Z")
        XCTAssertEqual(autumn.end, "2025-10-26T23:00:00.000Z")
        XCTAssertEqual(autumn.day.duration, 25 * 3_600)
    }

    @MainActor
    func testFutureStartAndMismatchedRunningWorklogDoNotAddTime() async throws {
        let clock = FakeClock()
        let state = DailyTotalsState(calendar: dailyCalendar())
        let future = WorklogItem(id: "future", taskId: firstTask.id, start: "2025-01-01T00:00:10.000Z", end: nil)
        let request = try XCTUnwrap(state.begin(clock: clock))
        state.accept(
            TrackerReport(snapshot: TrackerSnapshot(tasks: [firstTask], active: future), rows: []),
            requested: request, clock: clock)
        clock.uptime += 5
        clock.now.addTimeInterval(5)
        XCTAssertEqual(state.duration(taskID: firstTask.id, active: future, clock: clock), 0)
        clock.uptime += 8
        clock.now.addTimeInterval(8)
        XCTAssertEqual(state.duration(taskID: firstTask.id, active: future, clock: clock), 3)
        for changed in [
            WorklogItem(id: "other", taskId: firstTask.id, start: future.start, end: nil),
            WorklogItem(id: future.id, taskId: secondTask.id, start: future.start, end: nil),
            WorklogItem(id: future.id, taskId: firstTask.id, start: "2025-01-01T00:00:11.000Z", end: nil),
            WorklogItem(id: future.id, taskId: firstTask.id, start: future.start, end: "2025-01-01T00:00:12.000Z"),
        ] {
            XCTAssertEqual(state.duration(taskID: firstTask.id, active: changed, clock: clock), 0)
        }
    }

    @MainActor
    func testDayRolloverDiscardsYesterdayAndRejectsDelayedYesterdayReport() async throws {
        let clock = FakeClock()
        clock.now = dailyDate("2025-01-01T23:59:59.000Z")
        let state = DailyTotalsState(calendar: dailyCalendar())
        let request = try XCTUnwrap(state.begin(clock: clock))
        let report = TrackerReport(
            snapshot: TrackerSnapshot(tasks: [firstTask], active: activeWorklog), rows: [total(firstTask, 80)])
        state.accept(report, requested: request, clock: clock)
        clock.uptime += 2
        clock.now.addTimeInterval(2)
        XCTAssertNil(state.duration(taskID: firstTask.id, active: activeWorklog, clock: clock))
        state.accept(report, requested: request, clock: clock)
        XCTAssertEqual(state.status, .unavailable)
        XCTAssertTrue(state.pending)
        XCTAssertNil(state.duration(taskID: firstTask.id, active: activeWorklog, clock: clock))
    }

    @MainActor
    func testProjectionCapsAtDayEndWhenWallClockMovesBackward() async throws {
        let clock = FakeClock()
        clock.now = dailyDate("2025-01-01T23:59:59.000Z")
        let state = DailyTotalsState(calendar: dailyCalendar())
        let request = try XCTUnwrap(state.begin(clock: clock))
        state.accept(
            TrackerReport(
                snapshot: TrackerSnapshot(tasks: [firstTask], active: activeWorklog), rows: [total(firstTask, 10)]),
            requested: request, clock: clock)
        clock.uptime += 10
        clock.now.addTimeInterval(-100)
        XCTAssertEqual(state.duration(taskID: firstTask.id, active: activeWorklog, clock: clock), 11)
    }

    @MainActor
    func testWakeReanchorsToWallTimeAndPreservesReportBaseline() async throws {
        let clock = FakeClock()
        clock.now = dailyDate("2025-01-01T12:00:00.000Z")
        let state = DailyTotalsState(calendar: dailyCalendar())
        let request = try XCTUnwrap(state.begin(clock: clock))
        state.accept(
            TrackerReport(
                snapshot: TrackerSnapshot(tasks: [firstTask], active: activeWorklog), rows: [total(firstTask, 4)]),
            requested: request, clock: clock)
        clock.now.addTimeInterval(300)
        state.reanchor(clock: clock)
        XCTAssertEqual(state.duration(taskID: firstTask.id, active: activeWorklog, clock: clock), 304)
        clock.now.addTimeInterval(-400)
        state.reanchor(clock: clock)
        XCTAssertEqual(state.duration(taskID: firstTask.id, active: activeWorklog, clock: clock), 4)
    }

    @MainActor
    func testReportValidationRejectsInvalidRowsAndActiveIdentity() async {
        let state = DailyTotalsState(calendar: dailyCalendar())
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: nil)
        for rows in [
            [total(firstTask, -1)], [total(firstTask, 1), total(firstTask, 2)],
            [TaskReportTotal(taskId: "", durationMicroseconds: 1)],
        ] {
            XCTAssertThrowsError(try state.validate(TrackerReport(snapshot: snapshot, rows: rows)))
        }
        for active in [
            WorklogItem(id: "", taskId: firstTask.id, start: activeWorklog.start, end: nil),
            WorklogItem(id: "invalid", taskId: "", start: activeWorklog.start, end: nil),
            WorklogItem(id: "invalid", taskId: firstTask.id, start: "invalid", end: nil),
            WorklogItem(id: "invalid", taskId: firstTask.id, start: activeWorklog.start, end: activeWorklog.start),
        ] {
            XCTAssertThrowsError(
                try state.validate(
                    TrackerReport(snapshot: TrackerSnapshot(tasks: [firstTask], active: active), rows: [])))
        }
        XCTAssertThrowsError(
            try state.validate(
                TrackerReport(snapshot: TrackerSnapshot(tasks: [firstTask, firstTask], active: nil), rows: [])))
        XCTAssertNoThrow(try state.validate(TrackerReport(snapshot: snapshot, rows: [])))
    }

    @MainActor
    func testTotalsRemainValidWhenMetadataIsMissingAndRevisionMismatchFreezesProjection() async throws {
        let clock = FakeClock()
        let state = DailyTotalsState(calendar: dailyCalendar())
        let requested = try XCTUnwrap(state.begin(clock: clock))
        let snapshot = TrackerSnapshot(
            tasks: [], active: activeWorklog, tasksRevision: "epoch:1", trackingRevision: "epoch:1")
        let report = TrackerReport(
            snapshot: snapshot, rows: [total(secondTask, 12)], revision: "epoch:2", now: requested.now)
        try state.validate(report)
        state.accept(report, requested: requested, clock: clock)
        clock.uptime += 8
        clock.now.addTimeInterval(8)
        XCTAssertEqual(state.totalDuration(active: activeWorklog, clock: clock), 12)
        XCTAssertEqual(state.status, .cached)
        XCTAssertFalse(state.pending)
        XCTAssertFalse(
            TrackerSnapshot(tasks: [], active: nil, tasksRevision: "epoch:1", trackingRevision: "epoch:2")
                .resourcesAreCoherent)
    }

    @MainActor
    func testProjectionUsesEchoedCutoffOnlyAfterMatchingTrackingRevision() async throws {
        let clock = FakeClock()
        let state = DailyTotalsState(calendar: dailyCalendar())
        let requested = try XCTUnwrap(state.begin(clock: clock))
        let snapshot = TrackerSnapshot(
            tasks: [], active: activeWorklog, tasksRevision: "epoch:2", trackingRevision: "epoch:2")
        let cutoff = commandTimestamp(clock.now.addingTimeInterval(2))
        let report = TrackerReport(snapshot: snapshot, rows: [total(firstTask, 5)], revision: "epoch:2", now: cutoff)
        try state.validate(report)
        state.accept(report, requested: requested, clock: clock)
        clock.uptime += 3
        clock.now.addTimeInterval(3)
        XCTAssertEqual(state.totalDuration(active: activeWorklog, clock: clock), 8)
        XCTAssertEqual(state.status, .current)
        XCTAssertThrowsError(
            try state.validate(TrackerReport(snapshot: snapshot, rows: [], revision: "epoch:2", now: "invalid")))
    }

    @MainActor
    func testReportRequestedBeforeWakeRetainsSleepTimeWhenAcceptedAfterReanchor() async throws {
        let clock = FakeClock()
        let state = DailyTotalsState(calendar: dailyCalendar())
        let request = try XCTUnwrap(state.begin(clock: clock))
        clock.now.addTimeInterval(300)
        state.reanchor(clock: clock)
        state.accept(
            TrackerReport(
                snapshot: TrackerSnapshot(tasks: [firstTask], active: activeWorklog), rows: [total(firstTask, 4)]),
            requested: request, clock: clock)
        XCTAssertEqual(state.duration(taskID: firstTask.id, active: activeWorklog, clock: clock), 304)
        clock.uptime += 3
        clock.now.addTimeInterval(3)
        XCTAssertEqual(state.duration(taskID: firstTask.id, active: activeWorklog, clock: clock), 307)
    }

    @MainActor
    func testUnavailableFailureCachedFailureAndSourceClearHaveDistinctStates() async throws {
        let clock = FakeClock()
        let state = DailyTotalsState(calendar: dailyCalendar())
        XCTAssertNil(state.duration(taskID: firstTask.id, active: nil, clock: clock))
        _ = state.begin(clock: clock)
        XCTAssertEqual(state.status, .loading)
        state.fail(BridgeFailure(message: "No totals"), clock: clock)
        XCTAssertEqual(state.status, .unavailable)
        let request = try XCTUnwrap(state.begin(clock: clock))
        state.accept(
            TrackerReport(snapshot: TrackerSnapshot(tasks: [firstTask], active: nil), rows: [total(firstTask, 30)]),
            requested: request, clock: clock)
        _ = state.begin(clock: clock)
        XCTAssertEqual(state.status, .current)
        state.fail(BridgeFailure(message: "Offline"), clock: clock)
        XCTAssertEqual(state.status, .cached)
        XCTAssertEqual(state.duration(taskID: firstTask.id, active: nil, clock: clock), 30)
        state.clear(at: clock.now)
        XCTAssertNil(state.duration(taskID: firstTask.id, active: nil, clock: clock))
        XCTAssertEqual(state.status, .unavailable)
        XCTAssertNil(state.error)
    }
}

final class DailyTotalsSessionTests: XCTestCase {
    @MainActor
    func testAggregateFormatsExactMicrosecondSumsAtSecondAndMinuteBoundaries() async throws {
        for (count, microseconds, expectedSeconds, expectedText) in [
            (10, Int64(100_000), TimeInterval(1), "00:00:01"),
            (200, Int64(300_000), TimeInterval(60), "00:01:00"),
        ] {
            let fixture = Fixture(reports: true, calendar: dailyCalendar())
            defer { fixture.cleanup() }
            let tasks = (0..<count).map {
                TaskItem(id: "task-\($0)", name: "Task \($0)", archived: false, latestStart: nil)
            }
            let rows = tasks.map { TaskReportTotal(taskId: $0.id, durationMicroseconds: microseconds) }
            try await fixture.start(TrackerSnapshot(tasks: tasks, active: nil), rows: rows)
            XCTAssertEqual(fixture.session.totalDailyDuration, expectedSeconds)
            XCTAssertEqual(fixture.session.totalDailyDurationText, expectedText)
        }
    }

    @MainActor
    func testSessionAggregateRemainsUnavailableUntilAnEmptyReportIsLoaded() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        XCTAssertNil(fixture.session.totalDailyDuration)
        XCTAssertEqual(fixture.session.totalDailyDurationText, "Unavailable")
        fixture.session.start()
        let open = try await fixture.client.next()
        open.succeed(emptySnapshot)
        let report = try await fixture.client.next()
        XCTAssertTrue(fixture.session.tasks.isEmpty)
        XCTAssertNil(fixture.session.totalDailyDuration)
        report.succeed(TrackerReport(snapshot: emptySnapshot, rows: []))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.totalDailyDuration, 0)
        XCTAssertEqual(fixture.session.totalDailyDurationText, "00:00:00")
        fixture.clock.now.addTimeInterval(86_400)
        XCTAssertNil(fixture.session.totalDailyDuration)
        XCTAssertEqual(fixture.session.totalDailyDurationText, "Unavailable")
    }

    @MainActor
    func testDailyObserverNotifiesWhenFractionalSumChangesButIndividualTextsStayTheSame() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask, archivedTask], active: nil)
        let initialRows = snapshot.tasks.map { TaskReportTotal(taskId: $0.id, durationMicroseconds: 400_000) }
        try await fixture.start(snapshot, rows: initialRows)
        XCTAssertEqual(try XCTUnwrap(fixture.session.totalDailyDuration), 0.8, accuracy: 0.000001)
        XCTAssertEqual(fixture.session.totalDailyDurationText, "00:00:00")
        let observer = TrackerPresentationObserver(session: fixture.session)
        var totalTexts: [String] = []
        var contentChanges = 0
        observer.onDailyTotalsChange = { totalTexts.append(fixture.session.totalDailyDurationText) }
        observer.onContentChange = { contentChanges += 1 }
        fixture.session.onChange = { observer.update(from: fixture.session) }
        fixture.scheduler.poll?.fire()
        let report = try await fixture.client.next()
        let updatedRows = snapshot.tasks.map { TaskReportTotal(taskId: $0.id, durationMicroseconds: 600_000) }
        report.succeed(TrackerReport(snapshot: snapshot, rows: updatedRows))
        let finished = Task { @MainActor in while fixture.session.isBusy && !Task.isCancelled { await Task.yield() } }
        try await fixture.taskValue(finished)
        for task in snapshot.tasks {
            XCTAssertEqual(fixture.session.dailyDurationText(taskID: task.id), "00:00:00")
        }
        XCTAssertEqual(totalTexts, ["00:00:01"])
        XCTAssertEqual(contentChanges, 0)
    }

    @MainActor
    func testDailyObserverSurfacesChangedErrorWhileCachedStatusAndDurationStayTheSame() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: nil)
        try await fixture.start(snapshot, rows: [total(firstTask, 4)])
        let observer = TrackerPresentationObserver(session: fixture.session)
        var observedErrors: [String?] = []
        var observedStatuses: [DailyTotalsStatus] = []
        var observedTexts: [String] = []
        observer.onDailyTotalsChange = {
            observedErrors.append(fixture.session.dailyTotalsError)
            observedStatuses.append(fixture.session.dailyTotalsStatus)
            observedTexts.append(fixture.session.dailyDurationText(taskID: firstTask.id))
        }
        fixture.session.onChange = { observer.update(from: fixture.session) }
        for message in ["First report failure", "Second report failure"] {
            fixture.scheduler.poll?.fire()
            let report = try await fixture.client.next()
            report.fail(BridgeFailure(message: message))
            let finished = Task { @MainActor in while fixture.session.isBusy && !Task.isCancelled { await Task.yield() }
            }
            try await fixture.taskValue(finished)
        }
        XCTAssertEqual(observedErrors, ["First report failure", "Second report failure"])
        XCTAssertEqual(observedStatuses, [.cached, .cached])
        XCTAssertEqual(observedTexts, ["00:00:04", "00:00:04"])
    }

    @MainActor
    func testDailyObserverSurfacesNewDayDuringEmptyStartupBeforeReportBecomesAvailable() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        let observer = TrackerPresentationObserver(session: fixture.session)
        var observedDays: [Date?] = []
        var observedStatuses: [DailyTotalsStatus] = []
        var observedErrors: [String?] = []
        observer.onDailyTotalsChange = {
            observedDays.append(fixture.session.dailyTotalsDayStart)
            observedStatuses.append(fixture.session.dailyTotalsStatus)
            observedErrors.append(fixture.session.dailyTotalsError)
        }
        fixture.session.onChange = { observer.update(from: fixture.session) }
        fixture.session.start()
        let open = try await fixture.client.next()
        fixture.clock.now.addTimeInterval(86_400)
        open.succeed(emptySnapshot)
        let report = try await fixture.client.next()
        XCTAssertEqual(try XCTUnwrap(observedDays.first), fixture.clock.now)
        XCTAssertEqual(
            observedStatuses.first, .unavailable,
            "The changed day must reach the daily observer before loading changes its status.")
        XCTAssertNil(try XCTUnwrap(observedErrors.first))
        XCTAssertTrue(fixture.session.tasks.isEmpty)
        report.succeed(TrackerReport(snapshot: emptySnapshot, rows: []))
        let finished = Task { @MainActor in while fixture.session.isBusy && !Task.isCancelled { await Task.yield() } }
        try await fixture.taskValue(finished)
        XCTAssertEqual(observedStatuses.last, .current)
        XCTAssertTrue(observedDays.allSatisfy { $0 == fixture.clock.now })
    }

    @MainActor
    func testDelayedReportFailureAcrossMidnightImmediatelyRequestsTodaysTotals() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        fixture.clock.now = dailyDate("2025-01-01T23:59:58.000Z")
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: nil)
        try await fixture.start(snapshot, rows: [total(firstTask, 30)])
        fixture.session.refresh()
        let yesterday = try await fixture.client.next()
        fixture.clock.now.addTimeInterval(2)
        yesterday.fail(BridgeFailure(message: "Yesterday's report failed"))
        let today = try await fixture.client.next()
        guard case .report(_, let start, let end, let now) = today.operation else {
            return XCTFail("A delayed failure crossing midnight must request today's report.")
        }
        XCTAssertEqual(start, "2025-01-02T00:00:00.000Z")
        XCTAssertEqual(end, "2025-01-03T00:00:00.000Z")
        XCTAssertEqual(now, "2025-01-02T00:00:00.000Z")
        XCTAssertEqual(fixture.session.dailyTotalsDayStart, fixture.clock.now)
        XCTAssertNil(fixture.session.dailyDuration(taskID: firstTask.id))
        XCTAssertTrue(fixture.session.todayTasks.isEmpty)
        XCTAssertFalse(fixture.session.isStale)
        today.succeed(TrackerReport(snapshot: snapshot, rows: [total(firstTask, 2)]))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.dailyDuration(taskID: firstTask.id), 2)
        XCTAssertEqual(fixture.session.dailyTotalsStatus, .current)
        XCTAssertNil(fixture.session.dailyTotalsError)
    }

    @MainActor
    func testStaleRefreshClearsYesterdayDayAndErrorBeforeItsResponseArrives() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: nil)
        try await fixture.start(snapshot, rows: [total(firstTask, 4)])
        fixture.scheduler.poll?.fire()
        let failed = try await fixture.client.next()
        failed.fail(BridgeFailure(message: "Yesterday unavailable", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.dailyTotalsError, "Yesterday unavailable")
        fixture.clock.now.addTimeInterval(86_400)
        fixture.session.refresh()
        XCTAssertEqual(fixture.session.dailyTotalsDayStart, fixture.clock.now)
        XCTAssertNil(fixture.session.dailyTotalsError)
        XCTAssertTrue(fixture.session.todayTasks.isEmpty)
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .refresh(.local))
        refresh.succeed(snapshot)
        let report = try await fixture.client.next()
        report.succeed(TrackerReport(snapshot: snapshot, rows: []))
        try await fixture.settled()
    }

    @MainActor
    func testTrackingAcknowledgmentAcrossMidnightPublishesTodaysDayAndClearsYesterdayError() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        fixture.clock.now = dailyDate("2025-01-01T23:59:59.000Z")
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(snapshot, rows: [total(firstTask, 4)])
        fixture.scheduler.poll?.fire()
        let failed = try await fixture.client.next()
        failed.fail(BridgeFailure(message: "Yesterday totals unavailable"))
        try await fixture.settled()
        fixture.session.stopTracking(worklogID: activeWorklog.id)
        let stop = try await fixture.client.next()
        fixture.clock.now.addTimeInterval(1)
        var observedDays: [Date?] = []
        var observedErrors: [String?] = []
        fixture.session.onChange = {
            if fixture.session.active == nil {
                observedDays.append(fixture.session.dailyTotalsDayStart)
                observedErrors.append(fixture.session.dailyTotalsError)
            }
        }
        let stoppedSnapshot = TrackerSnapshot(tasks: [firstTask], active: nil)
        stop.succeed(stoppedSnapshot)
        let report = try await fixture.client.next()
        XCTAssertFalse(observedDays.isEmpty)
        XCTAssertTrue(
            observedDays.allSatisfy { $0 == fixture.clock.now },
            "The acknowledged snapshot must publish today's day before its report arrives.")
        XCTAssertTrue(
            observedErrors.allSatisfy { $0 == nil },
            "Yesterday's report error must disappear when the new day is published.")
        report.succeed(TrackerReport(snapshot: stoppedSnapshot, rows: [total(firstTask, 5)]))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
    }

    @MainActor
    func testHealthyReportAfterTimezoneChangeMovesRolloverToTheNewLocalMidnight() async throws {
        let originalZone = NSTimeZone.default
        defer { NSTimeZone.default = originalZone }
        NSTimeZone.default = TimeZone(identifier: "UTC")!
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        fixture.clock.now = dailyDate("2025-01-01T12:00:00.000Z")
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: nil)
        try await fixture.start(snapshot, rows: [total(firstTask, 4)])
        let initialRollover = try XCTUnwrap(fixture.scheduler.active.first { !$0.repeating && $0.tolerance == 0 })
        XCTAssertEqual(initialRollover.delay, 12 * 3_600)
        NSTimeZone.default = TimeZone(identifier: "America/New_York")!
        fixture.session.refresh()
        let changedZone = try await fixture.client.next()
        guard case .report(_, let start, let end, _) = changedZone.operation else {
            return XCTFail("The changed timezone must refresh the authoritative report.")
        }
        XCTAssertEqual(start, "2025-01-01T05:00:00.000Z")
        XCTAssertEqual(end, "2025-01-02T05:00:00.000Z")
        changedZone.succeed(TrackerReport(snapshot: snapshot, rows: [total(firstTask, 6)]))
        try await fixture.settled()
        let localMidnight = try XCTUnwrap(fixture.scheduler.active.first { !$0.repeating && $0.tolerance == 0 })
        XCTAssertEqual(
            localMidnight.delay, 17 * 3_600,
            "The rollover deadline must follow midnight in the new timezone.")
        fixture.clock.now.addTimeInterval(17 * 3_600)
        localMidnight.fire()
        XCTAssertNil(fixture.session.dailyDuration(taskID: firstTask.id))
        let nextDay = try await fixture.client.next()
        guard case .report(_, let nextStart, let nextEnd, _) = nextDay.operation else {
            return XCTFail("Local midnight must request the next day's report.")
        }
        XCTAssertEqual(nextStart, "2025-01-02T05:00:00.000Z")
        XCTAssertEqual(nextEnd, "2025-01-03T05:00:00.000Z")
        nextDay.succeed(TrackerReport(snapshot: snapshot, rows: []))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.todayTasks.isEmpty)
    }

    @MainActor
    func testTodaysTasksSortByNameAndThenByIDForMatchingNames() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        let laterID = TaskItem(id: "task-z", name: "Matching name", archived: false, latestStart: nil)
        let earlierID = TaskItem(id: "task-a", name: "Matching name", archived: true, latestStart: nil)
        let earlierName = TaskItem(id: "task-middle", name: "Earlier name", archived: false, latestStart: nil)
        let tasks = [laterID, earlierID, earlierName]
        try await fixture.start(TrackerSnapshot(tasks: tasks, active: nil), rows: tasks.map { total($0, 1) })
        XCTAssertEqual(fixture.session.todayTasks.map(\.id), [earlierName.id, earlierID.id, laterID.id])
    }

    @MainActor
    func testReportPublishesNewActiveSnapshotTogetherWithItsMatchingTotals() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        let tasks = [firstTask, secondTask]
        try await fixture.start(TrackerSnapshot(tasks: tasks, active: nil), rows: [total(firstTask, 10)])
        let newActive = WorklogItem(
            id: "new-active", taskId: secondTask.id,
            start: "2025-01-01T00:00:00.000Z", end: nil)
        var publishedTotals: [TimeInterval?] = []
        fixture.session.onChange = {
            if fixture.session.active == newActive {
                publishedTotals.append(fixture.session.dailyDuration(taskID: secondTask.id))
            }
        }
        fixture.scheduler.poll?.fire()
        let report = try await fixture.client.next()
        report.succeed(
            TrackerReport(
                snapshot: TrackerSnapshot(tasks: tasks, active: newActive),
                rows: [total(firstTask, 10), total(secondTask, 77)]))
        let finished = Task { @MainActor in while fixture.session.isBusy && !Task.isCancelled { await Task.yield() } }
        try await fixture.taskValue(finished)
        XCTAssertFalse(publishedTotals.isEmpty)
        XCTAssertTrue(
            publishedTotals.allSatisfy { $0 == 77 },
            "Observers must receive the new active worklog and its report totals together.")
    }

    @MainActor
    func testHealthyPollUsesReportSnapshotAndRetainsAllTaskTotalsWithoutHistoryPagination() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [secondTask, archivedTask, firstTask], active: activeWorklog)
        try await fixture.start(snapshot, rows: [total(firstTask, 20), total(archivedTask, 50)])
        XCTAssertEqual(fixture.session.todayTasks.map(\.id), [archivedTask.id, firstTask.id])
        XCTAssertEqual(fixture.session.dailyDurationText(taskID: firstTask.id), "00:00:20")
        XCTAssertEqual(fixture.session.dailyDuration(taskID: secondTask.id), 0)
        XCTAssertTrue(fixture.session.worklogs.isEmpty)
        let count = fixture.client.operations.count
        fixture.scheduler.poll?.fire()
        let poll = try await fixture.client.next()
        guard case .report = poll.operation else { return XCTFail("Healthy polls must request the report directly.") }
        XCTAssertEqual(fixture.session.dailyTotalsStatus, .current)
        poll.succeed(TrackerReport(snapshot: snapshot, rows: [total(firstTask, 25), total(archivedTask, 50)]))
        try await fixture.settled()
        XCTAssertEqual(fixture.client.operations.count, count + 1)
        XCTAssertEqual(fixture.session.dailyDuration(taskID: firstTask.id), 25)
    }

    @MainActor
    func testDisplayTicksUseDedicatedDailyNotificationAndDoNotInvalidateTaskLists() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        fixture.session.setWindowVisible(true)
        try await fixture.start(
            TrackerSnapshot(tasks: [firstTask], active: activeWorklog), rows: [total(firstTask, 10)])
        let observer = TrackerPresentationObserver(session: fixture.session)
        var content = 0
        var daily = 0
        observer.onContentChange = { content += 1 }
        observer.onDailyTotalsChange = { daily += 1 }
        fixture.session.onChange = { observer.update(from: fixture.session) }
        let count = fixture.client.operations.count
        for _ in 0..<3 {
            fixture.clock.now.addTimeInterval(1)
            fixture.clock.uptime += 1
            fixture.scheduler.display?.fire()
        }
        XCTAssertEqual(daily, 3)
        XCTAssertEqual(content, 0)
        XCTAssertEqual(fixture.client.operations.count, count)
        observer.update(from: fixture.session)
        XCTAssertEqual(daily, 3)
    }

    @MainActor
    func testFailureRequiringRefreshDisablesWritesAndRecoversWithRefreshBeforeReport() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(snapshot, rows: [total(firstTask, 10)])
        fixture.scheduler.poll?.fire()
        let report = try await fixture.client.next()
        report.fail(BridgeFailure(message: "State changed", requiresRefresh: true))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.isStale)
        XCTAssertFalse(fixture.session.canStopTracking)
        XCTAssertEqual(fixture.session.dailyTotalsStatus, .cached)
        fixture.session.refresh()
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .refresh(.local))
        refresh.succeed(snapshot)
        let replacement = try await fixture.client.next()
        guard case .report = replacement.operation else {
            return XCTFail("Recovery must reload totals before history.")
        }
        XCTAssertTrue(fixture.session.canStopTracking)
        replacement.succeed(TrackerReport(snapshot: snapshot, rows: [total(firstTask, 15)]))
        try await fixture.settled()
        XCTAssertFalse(fixture.session.isStale)
        XCTAssertTrue(fixture.session.canStopTracking)
        XCTAssertEqual(fixture.session.dailyTotalsStatus, .current)
    }

    @MainActor
    func testUnavailableProtocolAndUncertainReportFailuresRequireSnapshotRecovery() async throws {
        let failures = [
            BridgeFailure(message: "Report offline", kind: "unavailable"),
            BridgeFailure(message: "Report incompatible", kind: "protocol"),
            BridgeFailure(message: "Report state uncertain", uncertain: true),
        ]
        for failure in failures {
            let fixture = Fixture(reports: true, calendar: dailyCalendar())
            defer { fixture.cleanup() }
            let snapshot = TrackerSnapshot(tasks: [firstTask], active: nil)
            try await fixture.start(snapshot, rows: [total(firstTask, 4)])
            fixture.scheduler.poll?.fire()
            let failed = try await fixture.client.next()
            failed.fail(failure)
            try await fixture.settled()
            XCTAssertEqual(fixture.session.dailyTotalsStatus, .cached, failure.message)
            XCTAssertEqual(fixture.session.dailyDuration(taskID: firstTask.id), 4, failure.message)
            XCTAssertTrue(fixture.session.isStale, failure.message)
            XCTAssertFalse(fixture.session.canStartSelectedTask, failure.message)
            XCTAssertFalse(fixture.session.canStopTracking, failure.message)
            if failure.kind == "protocol" {
                XCTAssertNil(fixture.scheduler.poll)
                XCTAssertEqual(fixture.session.connectionStatusText, "Incompatible server")
            } else {
                XCTAssertNotNil(fixture.scheduler.poll)
            }
            fixture.session.refresh()
            let refresh = try await fixture.client.next()
            XCTAssertEqual(refresh.operation, .refresh(.local), failure.message)
            refresh.succeed(snapshot)
            let report = try await fixture.client.next()
            guard case .report = report.operation else {
                return XCTFail("Snapshot recovery must precede the next report.")
            }
            XCTAssertTrue(fixture.session.canStartSelectedTask)
            report.succeed(TrackerReport(snapshot: snapshot, rows: [total(firstTask, 8)]))
            try await fixture.settled()
            XCTAssertFalse(fixture.session.isStale, failure.message)
            XCTAssertTrue(fixture.session.canStartSelectedTask, failure.message)
            XCTAssertEqual(fixture.session.dailyTotalsStatus, .current, failure.message)
        }
    }

    @MainActor
    func testMalformedReportFailsAsProtocolBeforeAdoptingItsSnapshot() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(snapshot, rows: [total(firstTask, 4)])
        fixture.scheduler.poll?.fire()
        let report = try await fixture.client.next()
        let replacementActive = WorklogItem(
            id: "replacement", taskId: secondTask.id,
            start: "2025-01-01T00:00:00.000Z", end: nil)
        report.succeed(
            TrackerReport(
                snapshot: TrackerSnapshot(tasks: [secondTask], active: replacementActive),
                rows: [total(firstTask, 8), total(firstTask, 2)]))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.tasks, [firstTask])
        XCTAssertEqual(fixture.session.active, activeWorklog)
        XCTAssertEqual(fixture.session.dailyDuration(taskID: firstTask.id), 4)
        XCTAssertEqual(fixture.session.dailyTotalsStatus, .cached)
        XCTAssertTrue(fixture.session.isStale)
        XCTAssertFalse(fixture.session.canStopTracking)
        XCTAssertEqual(fixture.session.connectionStatusText, "Incompatible server")
        XCTAssertNil(fixture.scheduler.poll)
    }

    @MainActor
    func testBothLocalDayTimersClearYesterdayWithoutRetryingProtocolFailure() async throws {
        for useDisplayTimer in [false, true] {
            let fixture = Fixture(reports: true, calendar: dailyCalendar())
            defer { fixture.cleanup() }
            fixture.session.setWindowVisible(true)
            let snapshot = TrackerSnapshot(tasks: [firstTask], active: activeWorklog)
            try await fixture.start(snapshot, rows: [total(firstTask, 4)])
            let display = try XCTUnwrap(fixture.scheduler.display)
            let rollover = try XCTUnwrap(fixture.scheduler.active.first { !$0.repeating && $0.tolerance == 0 })
            fixture.scheduler.poll?.fire()
            let report = try await fixture.client.next()
            report.fail(BridgeFailure(message: "Report incompatible", kind: "protocol"))
            try await fixture.settled()
            let operationCount = fixture.client.operations.count
            fixture.clock.now.addTimeInterval(86_400)
            fixture.clock.uptime += 86_400
            if useDisplayTimer { display.fire() } else { rollover.fire() }
            XCTAssertEqual(fixture.client.operations.count, operationCount)
            XCTAssertFalse(fixture.session.isBusy)
            XCTAssertNil(fixture.session.dailyDuration(taskID: firstTask.id))
            XCTAssertTrue(fixture.session.todayTasks.isEmpty)
            XCTAssertEqual(fixture.session.dailyTotalsStatus, .unavailable)
            XCTAssertEqual(fixture.session.connectionStatusText, "Incompatible server")
            XCTAssertNil(fixture.scheduler.poll)
        }
    }

    @MainActor
    func testSuccessfulSourceChangeClearsTotalsButFailedCandidatePreservesThem() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: nil)
        try await fixture.start(snapshot, rows: [total(firstTask, 100)])
        let failed = Task { await fixture.session.connect(serverSettings) }
        let failure = try await fixture.client.next()
        failure.fail(BridgeFailure(message: "Candidate offline"))
        let failedResult = try await fixture.taskValue(failed)
        XCTAssertFalse(failedResult)
        XCTAssertEqual(fixture.session.dailyDuration(taskID: firstTask.id), 100)
        let connected = Task { await fixture.session.connect(serverSettings) }
        let connect = try await fixture.client.next()
        connect.succeed(snapshot)
        let connectedResult = try await fixture.taskValue(connected)
        XCTAssertTrue(connectedResult)
        let report = try await fixture.client.next()
        XCTAssertNil(fixture.session.dailyDuration(taskID: firstTask.id))
        guard case .report(let settings, _, _, _) = report.operation else {
            return XCTFail("New source must load its own report.")
        }
        XCTAssertEqual(settings, serverSettings)
        report.succeed(TrackerReport(snapshot: snapshot, rows: []))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.dailyDuration(taskID: firstTask.id), 0)
    }

    @MainActor
    func testTrackingCommandQueuesReportBeforeHistoryAndNeverAddsWholeActiveElapsed() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(snapshot, rows: [total(firstTask, 5)])
        fixture.session.stopTracking(worklogID: activeWorklog.id)
        let stop = try await fixture.client.next()
        stop.succeed(TrackerSnapshot(tasks: [firstTask], active: nil))
        let report = try await fixture.client.next()
        guard case .report = report.operation else { return XCTFail("Tracking changes must report before history.") }
        XCTAssertEqual(fixture.session.dailyTotalsStatus, .cached)
        report.succeed(
            TrackerReport(snapshot: TrackerSnapshot(tasks: [firstTask], active: nil), rows: [total(firstTask, 7)]))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.dailyDuration(taskID: firstTask.id), 7)
    }

    @MainActor
    func testMidnightTimerCoalescesReportsAndIgnoresLateResultAfterShutdown() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        fixture.clock.now = dailyDate("2025-01-01T23:59:59.000Z")
        fixture.session.setWindowVisible(true)
        try await fixture.start(TrackerSnapshot(tasks: [firstTask], active: activeWorklog), rows: [total(firstTask, 5)])
        fixture.clock.now.addTimeInterval(1)
        fixture.clock.uptime += 1
        fixture.scheduler.display?.fire()
        let report = try await fixture.client.next()
        guard case .report(_, let start, _, _) = report.operation else {
            return XCTFail("Midnight must request the new day.")
        }
        XCTAssertEqual(start, "2025-01-02T00:00:00.000Z")
        XCTAssertNil(fixture.session.dailyDuration(taskID: firstTask.id))
        let count = fixture.client.operations.count
        fixture.scheduler.display?.fire()
        XCTAssertEqual(fixture.client.operations.count, count)
        fixture.session.shutdown()
        report.succeed(TrackerReport(snapshot: emptySnapshot, rows: []))
        await Task.yield()
        XCTAssertEqual(fixture.session.tasks, [firstTask])
        XCTAssertTrue(fixture.scheduler.active.isEmpty)
        XCTAssertFalse(fixture.session.isBusy)
    }

    @MainActor
    func testIdleMidnightTimerClearsYesterdayAndRequestsTodaysReport() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        fixture.clock.now = dailyDate("2025-01-01T23:59:58.000Z")
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: nil)
        try await fixture.start(snapshot, rows: [total(firstTask, 30)])
        XCTAssertNil(fixture.scheduler.display)
        let rollover = try XCTUnwrap(fixture.scheduler.active.first { !$0.repeating && $0.delay == 2 })
        fixture.clock.now.addTimeInterval(2)
        rollover.fire()
        let report = try await fixture.client.next()
        guard case .report(_, let start, _, _) = report.operation else {
            return XCTFail("Idle midnight must fetch the new day.")
        }
        XCTAssertEqual(start, "2025-01-02T00:00:00.000Z")
        XCTAssertTrue(fixture.session.todayTasks.isEmpty)
        report.succeed(TrackerReport(snapshot: snapshot, rows: []))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.dailyDuration(taskID: firstTask.id), 0)
    }

    @MainActor
    func testDelayedReportAcrossMidnightCannotRestoreYesterdayAndQueuesOneReplacement() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        fixture.clock.now = dailyDate("2025-01-01T23:59:58.000Z")
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: nil)
        try await fixture.start(snapshot, rows: [total(firstTask, 30)])
        let rollover = try XCTUnwrap(fixture.scheduler.active.first { !$0.repeating && $0.delay == 2 })
        fixture.session.refresh()
        let yesterday = try await fixture.client.next()
        fixture.clock.now.addTimeInterval(2)
        rollover.fire()
        XCTAssertNil(fixture.session.dailyDuration(taskID: firstTask.id))
        yesterday.succeed(TrackerReport(snapshot: snapshot, rows: [total(firstTask, 40)]))
        let today = try await fixture.client.next()
        guard case .report(_, let start, _, _) = today.operation else {
            return XCTFail("Delayed report must be replaced for today.")
        }
        XCTAssertEqual(start, "2025-01-02T00:00:00.000Z")
        XCTAssertNil(fixture.session.dailyDuration(taskID: firstTask.id))
        today.succeed(TrackerReport(snapshot: snapshot, rows: [total(firstTask, 2)]))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.dailyDuration(taskID: firstTask.id), 2)
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .report = $0 { return true }; return false
            }.count, 3)
    }

    @MainActor
    func testSleepCancelsRolloverAndWakeReanchorsBeforeRefreshingReport() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        fixture.session.setWindowVisible(true)
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(snapshot, rows: [total(firstTask, 4)])
        let timers = fixture.scheduler.active
        fixture.session.sleep()
        XCTAssertTrue(timers.allSatisfy(\.cancelled))
        XCTAssertTrue(fixture.scheduler.active.isEmpty)
        fixture.clock.now.addTimeInterval(300)
        fixture.session.wake()
        let wake = try await fixture.client.next()
        guard case .report = wake.operation else {
            return XCTFail("Healthy wake must refresh the authoritative report.")
        }
        XCTAssertEqual(fixture.session.dailyDuration(taskID: firstTask.id), 304)
        wake.succeed(TrackerReport(snapshot: snapshot, rows: [total(firstTask, 304)]))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.dailyDuration(taskID: firstTask.id), 304)
        let count = fixture.client.operations.count
        timers.forEach { $0.deliverQueuedAction() }
        XCTAssertEqual(fixture.client.operations.count, count)
    }

    @MainActor
    func testUnchangedHealthyPollDoesNotNotifyContentOrDailyTotalsAndFailureNotifiesStatus() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: nil)
        try await fixture.start(snapshot, rows: [total(firstTask, 4)])
        let observer = TrackerPresentationObserver(session: fixture.session)
        var content = 0
        var daily = 0
        observer.onContentChange = { content += 1 }
        observer.onDailyTotalsChange = { daily += 1 }
        fixture.session.onChange = { observer.update(from: fixture.session) }
        fixture.scheduler.poll?.fire()
        let poll = try await fixture.client.next()
        poll.succeed(TrackerReport(snapshot: snapshot, rows: [total(firstTask, 4)]))
        let finished = Task { @MainActor in while fixture.session.isBusy && !Task.isCancelled { await Task.yield() } }
        try await fixture.taskValue(finished)
        XCTAssertEqual(content, 0)
        XCTAssertEqual(daily, 0)
        fixture.scheduler.poll?.fire()
        let failed = try await fixture.client.next()
        failed.fail(BridgeFailure(message: "Totals unavailable"))
        let failureFinished = Task { @MainActor in
            while fixture.session.isBusy && !Task.isCancelled { await Task.yield() }
        }
        try await fixture.taskValue(failureFinished)
        XCTAssertEqual(content, 1)
        XCTAssertEqual(daily, 1)
        XCTAssertEqual(fixture.session.dailyDuration(taskID: firstTask.id), 4)
        XCTAssertFalse(fixture.session.isStale)
        XCTAssertTrue(fixture.session.canStartSelectedTask)
    }
}
