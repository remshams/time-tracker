import Foundation
import XCTest
@testable import TrackerClient

private func dailyCalendar(_ zone: String = "UTC") -> Calendar {
    var calendar = Calendar(identifier: .gregorian)
    calendar.timeZone = TimeZone(identifier: zone)!
    return calendar
}

@MainActor
private func dailyDate(_ value: String) -> Date { parseTimestamp(value)! }

private func total(_ task: TaskItem, _ seconds: Int64) -> TaskReportTotal {
    TaskReportTotal(taskId: task.id, durationMicroseconds: seconds * 1_000_000)
}

final class DailyTotalsTests: XCTestCase {
    @MainActor
    func testAggregateHandlesMicrosecondSumAboveInt64WithoutOverflow() async throws {
        let clock = FakeClock()
        let state = DailyTotalsStore(calendar: dailyCalendar())
        let request = try XCTUnwrap(state.begin(clock: clock))
        let rows = [firstTask, secondTask].map {
            TaskReportTotal(taskId: $0.id, durationMicroseconds: Int64.max)
        }
        state.accept(
            TaskListTotalsRefresh(
                taskList: TaskListResources(tasks: [firstTask, secondTask], active: nil), rows: rows),
            requested: request, clock: clock)
        XCTAssertEqual(state.totalDuration(active: nil, clock: clock), 18_446_744_073_709.55)
    }

    @MainActor
    func testAggregateDistinguishesUnavailableDataFromLoadedEmptyAndCachedEmptyReports() async throws {
        let clock = FakeClock()
        let state = DailyTotalsStore(calendar: dailyCalendar())
        XCTAssertNil(state.totalDuration(active: nil, clock: clock))
        let request = try XCTUnwrap(state.begin(clock: clock))
        XCTAssertNil(state.totalDuration(active: nil, clock: clock))
        state.accept(
            TaskListTotalsRefresh(taskList: emptyTaskListResources, rows: []), requested: request, clock: clock)
        XCTAssertEqual(state.totalDuration(active: nil, clock: clock), 0)
        state.fail(BridgeFailure(message: "Report unavailable"), clock: clock)
        XCTAssertEqual(state.loadStatus, .cached)
        XCTAssertEqual(state.totalDuration(active: nil, clock: clock), 0)
        clock.now.addTimeInterval(86_400)
        XCTAssertNil(state.totalDuration(active: nil, clock: clock))
    }

    @MainActor
    func testAggregateIncludesArchivedRowsAndProjectsTheMatchingRunningWorklogOnce() async throws {
        let clock = FakeClock()
        let state = DailyTotalsStore(calendar: dailyCalendar())
        let request = try XCTUnwrap(state.begin(clock: clock))
        state.accept(
            TaskListTotalsRefresh(
                taskList: TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: activeWorklog),
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
    func testPublishedMenuValuesNotifyOnlyWhenTheyChange() async throws {
        let clock = FakeClock()
        let store = DailyTotalsStore(calendar: dailyCalendar())
        let tasks = [firstTask, secondTask]
        let probe = ObservationProbe {
            _ = store.dayTotal
            _ = store.totalsExplanation
            _ = store.taskDurations
        }
        store.publish(tasks: tasks, active: activeWorklog, clock: clock)
        XCTAssertFalse(probe.consume(), "Totals without a report keep their initial values.")
        XCTAssertNil(store.dayTotal)
        XCTAssertEqual(store.totalsExplanation, "Today's total is unavailable")
        XCTAssertEqual(store.taskDurations, [:])
        let request = try XCTUnwrap(store.begin(clock: clock))
        store.accept(
            TaskListTotalsRefresh(
                taskList: TaskListResources(tasks: tasks, active: activeWorklog),
                rows: [total(firstTask, 20), total(archivedTask, 50)]),
            requested: request, clock: clock)
        store.publish(tasks: tasks, active: activeWorklog, clock: clock)
        XCTAssertTrue(probe.consume())
        XCTAssertEqual(store.dayTotal, 70)
        XCTAssertEqual(store.totalsExplanation, "Time logged today in your local time zone")
        XCTAssertEqual(store.taskDurations, [firstTask.id: 20, secondTask.id: 0])
        store.publish(tasks: tasks, active: activeWorklog, clock: clock)
        XCTAssertFalse(probe.consume(), "An unchanged publish must not redraw the menu bar.")
        clock.now.addTimeInterval(7)
        clock.uptime += 7
        store.publish(tasks: tasks, active: activeWorklog, clock: clock)
        XCTAssertTrue(probe.consume())
        XCTAssertEqual(store.dayTotal, 77)
        XCTAssertEqual(store.taskDurations[firstTask.id], 27)
        store.fail(BridgeFailure(message: "Report unavailable"), clock: clock)
        store.publish(tasks: tasks, active: activeWorklog, clock: clock)
        XCTAssertTrue(probe.consume())
        XCTAssertEqual(
            store.totalsExplanation, "Today's total uses cached tracker state. Running time may be unconfirmed.")
    }

    @MainActor
    func testAggregateProjectsRunningTaskWhenItsReportRowIsAbsent() async throws {
        let clock = FakeClock()
        let state = DailyTotalsStore(calendar: dailyCalendar())
        let request = try XCTUnwrap(state.begin(clock: clock))
        state.accept(
            TaskListTotalsRefresh(
                taskList: TaskListResources(tasks: [firstTask, archivedTask], active: activeWorklog),
                rows: [total(archivedTask, 50)]), requested: request, clock: clock)
        clock.uptime += 3
        clock.now.addTimeInterval(3)
        XCTAssertEqual(state.totalDuration(active: activeWorklog, clock: clock), 53)
    }

    @MainActor
    func testReportIncludesRunningTimeOnceAndAddsOnlyTimeAfterCapturedCutoff() async throws {
        let clock = FakeClock()
        clock.now = dailyDate("2025-01-01T12:00:00.123Z")
        let state = DailyTotalsStore(calendar: dailyCalendar())
        let request = try XCTUnwrap(state.begin(clock: clock))
        XCTAssertEqual(request.now, "2025-01-01T12:00:00.123Z")
        clock.uptime += 7
        clock.now.addTimeInterval(7)
        state.accept(
            TaskListTotalsRefresh(
                taskList: TaskListResources(tasks: [firstTask], active: activeWorklog),
                rows: [total(firstTask, 43_200)]), requested: request, clock: clock)
        XCTAssertEqual(state.duration(taskID: firstTask.id, active: activeWorklog, clock: clock), 43_207)
        XCTAssertEqual(state.duration(taskID: secondTask.id, active: activeWorklog, clock: clock), 0)
    }

    @MainActor
    func testLocalDayIntervalsUseTwentyThreeAndTwentyFiveHoursAcrossDaylightSaving() async throws {
        let clock = FakeClock()
        let state = DailyTotalsStore(calendar: dailyCalendar("Europe/Berlin"))
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
        let state = DailyTotalsStore(calendar: dailyCalendar())
        let future = WorklogItem(id: "future", taskId: firstTask.id, start: "2025-01-01T00:00:10.000Z", end: nil)
        let request = try XCTUnwrap(state.begin(clock: clock))
        state.accept(
            TaskListTotalsRefresh(taskList: TaskListResources(tasks: [firstTask], active: future), rows: []),
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
        let state = DailyTotalsStore(calendar: dailyCalendar())
        let request = try XCTUnwrap(state.begin(clock: clock))
        let report = TaskListTotalsRefresh(
            taskList: TaskListResources(tasks: [firstTask], active: activeWorklog), rows: [total(firstTask, 80)])
        state.accept(report, requested: request, clock: clock)
        clock.uptime += 2
        clock.now.addTimeInterval(2)
        XCTAssertNil(state.duration(taskID: firstTask.id, active: activeWorklog, clock: clock))
        state.accept(report, requested: request, clock: clock)
        XCTAssertEqual(state.loadStatus, .unavailable)
        XCTAssertNil(state.duration(taskID: firstTask.id, active: activeWorklog, clock: clock))
    }

    @MainActor
    func testProjectionCapsAtDayEndWhenWallClockMovesBackward() async throws {
        let clock = FakeClock()
        clock.now = dailyDate("2025-01-01T23:59:59.000Z")
        let state = DailyTotalsStore(calendar: dailyCalendar())
        let request = try XCTUnwrap(state.begin(clock: clock))
        state.accept(
            TaskListTotalsRefresh(
                taskList: TaskListResources(tasks: [firstTask], active: activeWorklog), rows: [total(firstTask, 10)]),
            requested: request, clock: clock)
        clock.uptime += 10
        clock.now.addTimeInterval(-100)
        XCTAssertEqual(state.duration(taskID: firstTask.id, active: activeWorklog, clock: clock), 11)
    }

    @MainActor
    func testWakeReanchorsToWallTimeAndPreservesReportBaseline() async throws {
        let clock = FakeClock()
        clock.now = dailyDate("2025-01-01T12:00:00.000Z")
        let state = DailyTotalsStore(calendar: dailyCalendar())
        let request = try XCTUnwrap(state.begin(clock: clock))
        state.accept(
            TaskListTotalsRefresh(
                taskList: TaskListResources(tasks: [firstTask], active: activeWorklog), rows: [total(firstTask, 4)]),
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
        let state = DailyTotalsStore(calendar: dailyCalendar())
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        for rows in [
            [total(firstTask, -1)], [total(firstTask, 1), total(firstTask, 2)],
            [TaskReportTotal(taskId: "", durationMicroseconds: 1)],
        ] {
            XCTAssertThrowsError(try state.validate(TaskListTotalsRefresh(taskList: resources, rows: rows)))
        }
        for active in [
            WorklogItem(id: "", taskId: firstTask.id, start: activeWorklog.start, end: nil),
            WorklogItem(id: "invalid", taskId: "", start: activeWorklog.start, end: nil),
            WorklogItem(id: "invalid", taskId: firstTask.id, start: "invalid", end: nil),
            WorklogItem(id: "invalid", taskId: firstTask.id, start: activeWorklog.start, end: activeWorklog.start),
        ] {
            XCTAssertThrowsError(
                try state.validate(
                    TaskListTotalsRefresh(taskList: TaskListResources(tasks: [firstTask], active: active), rows: [])))
        }
        XCTAssertNoThrow(
            try state.validate(
                TaskListTotalsRefresh(
                    taskList: TaskListResources(tasks: [firstTask, firstTask], active: nil), rows: [])))
        XCTAssertNoThrow(try state.validate(TaskListTotalsRefresh(taskList: resources, rows: [])))
    }

    @MainActor
    func testTotalsRemainValidAndProjectRunningTimeWhenTaskMetadataIsMissing() async throws {
        let clock = FakeClock()
        let state = DailyTotalsStore(calendar: dailyCalendar())
        let requested = try XCTUnwrap(state.begin(clock: clock))
        let report = TaskListTotalsRefresh(
            taskList: TaskListResources(tasks: [], active: activeWorklog), rows: [total(secondTask, 12)],
            now: requested.now)
        try state.validate(report)
        state.accept(report, requested: requested, clock: clock)
        clock.uptime += 8
        clock.now.addTimeInterval(8)
        XCTAssertEqual(state.totalDuration(active: activeWorklog, clock: clock), 20)
        XCTAssertEqual(state.loadStatus, .current)
    }

    @MainActor
    func testProjectionUsesEchoedCutoff() async throws {
        let clock = FakeClock()
        let state = DailyTotalsStore(calendar: dailyCalendar())
        let requested = try XCTUnwrap(state.begin(clock: clock))
        let resources = TaskListResources(tasks: [], active: activeWorklog)
        let cutoff = formatTimestamp(clock.now.addingTimeInterval(2))
        let report = TaskListTotalsRefresh(taskList: resources, rows: [total(firstTask, 5)], now: cutoff)
        try state.validate(report)
        state.accept(report, requested: requested, clock: clock)
        clock.uptime += 3
        clock.now.addTimeInterval(3)
        XCTAssertEqual(state.totalDuration(active: activeWorklog, clock: clock), 8)
        XCTAssertEqual(state.loadStatus, .current)
        XCTAssertThrowsError(
            try state.validate(
                TaskListTotalsRefresh(taskList: resources, rows: [], now: "invalid"))
        )
    }

    @MainActor
    func testReportRequestedBeforeWakeRetainsSleepTimeWhenAcceptedAfterReanchor() async throws {
        let clock = FakeClock()
        let state = DailyTotalsStore(calendar: dailyCalendar())
        let request = try XCTUnwrap(state.begin(clock: clock))
        clock.now.addTimeInterval(300)
        state.reanchor(clock: clock)
        state.accept(
            TaskListTotalsRefresh(
                taskList: TaskListResources(tasks: [firstTask], active: activeWorklog), rows: [total(firstTask, 4)]),
            requested: request, clock: clock)
        XCTAssertEqual(state.duration(taskID: firstTask.id, active: activeWorklog, clock: clock), 304)
        clock.uptime += 3
        clock.now.addTimeInterval(3)
        XCTAssertEqual(state.duration(taskID: firstTask.id, active: activeWorklog, clock: clock), 307)
    }

    @MainActor
    func testUnavailableFailureCachedFailureAndSourceClearHaveDistinctStates() async throws {
        let clock = FakeClock()
        let state = DailyTotalsStore(calendar: dailyCalendar())
        XCTAssertNil(state.duration(taskID: firstTask.id, active: nil, clock: clock))
        _ = state.begin(clock: clock)
        XCTAssertEqual(state.loadStatus, .loading)
        state.fail(BridgeFailure(message: "No totals"), clock: clock)
        XCTAssertEqual(state.loadStatus, .unavailable)
        let request = try XCTUnwrap(state.begin(clock: clock))
        state.accept(
            TaskListTotalsRefresh(
                taskList: TaskListResources(tasks: [firstTask], active: nil), rows: [total(firstTask, 30)]),
            requested: request, clock: clock)
        _ = state.begin(clock: clock)
        XCTAssertEqual(state.loadStatus, .current)
        state.fail(BridgeFailure(message: "Offline"), clock: clock)
        XCTAssertEqual(state.loadStatus, .cached)
        XCTAssertEqual(state.duration(taskID: firstTask.id, active: nil, clock: clock), 30)
        state.clear(at: clock.now)
        XCTAssertNil(state.duration(taskID: firstTask.id, active: nil, clock: clock))
        XCTAssertEqual(state.loadStatus, .unavailable)
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
            try await fixture.start(TaskListResources(tasks: tasks, active: nil), rows: rows)
            XCTAssertEqual(fixture.totalDuration, expectedSeconds)
            XCTAssertEqual(fixture.totalDurationText, expectedText)
        }
    }

    @MainActor
    func testSessionAggregateRemainsUnavailableUntilAnEmptyReportIsLoaded() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        XCTAssertNil(fixture.totalDuration)
        XCTAssertEqual(fixture.totalDurationText, "Unavailable")
        fixture.sync.start()
        let open = try await fixture.client.next()
        open.succeed(emptyTaskListResources)
        let report = try await fixture.client.next()
        XCTAssertTrue(fixture.session.tasks.isEmpty)
        XCTAssertNil(fixture.totalDuration)
        report.succeed(TaskListTotalsRefresh(taskList: emptyTaskListResources, rows: []))
        try await fixture.settled()
        XCTAssertEqual(fixture.totalDuration, 0)
        XCTAssertEqual(fixture.totalDurationText, "00:00:00")
        fixture.clock.now.addTimeInterval(86_400)
        XCTAssertNil(fixture.totalDuration)
        XCTAssertEqual(fixture.totalDurationText, "Unavailable")
    }

    @MainActor
    func testFractionalSumChangeKeepsTaskTotalLabelsAndListsUnchanged() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, archivedTask], active: nil)
        let initialRows = resources.catalog.value.map { TaskReportTotal(taskId: $0.id, durationMicroseconds: 400_000) }
        try await fixture.start(resources, rows: initialRows)
        XCTAssertEqual(try XCTUnwrap(fixture.totalDuration), 0.8, accuracy: 0.000001)
        XCTAssertEqual(fixture.totalDurationText, "00:00:00")
        let harness = PresentationHarness(session: fixture.session)
        let totals = TaskTotalProbes(harness.dailyTotals, taskIDs: resources.catalog.value.map(\.id))
        let contentChanges = harness.counter(PresentationReads.content)
        fixture.scheduler.poll?.fire()
        let report = try await fixture.client.next()
        let updatedRows = resources.catalog.value.map { TaskReportTotal(taskId: $0.id, durationMicroseconds: 600_000) }
        report.succeed(TaskListTotalsRefresh(taskList: resources, rows: updatedRows))
        let finished = Task { @MainActor in
            while fixture.client.hasPendingRequests && !Task.isCancelled { await Task.yield() }
        }
        try await fixture.taskValue(finished)
        for task in resources.catalog.value {
            XCTAssertEqual(fixture.durationText(taskID: task.id), "00:00:00")
        }
        XCTAssertEqual(fixture.totalDurationText, "00:00:01")
        XCTAssertEqual(totals.changed(), [])
        XCTAssertEqual(contentChanges.count, 0)
    }

    @MainActor
    func testChangedErrorKeepsCachedStatusDurationAndTaskTotalLabels() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources, rows: [total(firstTask, 4)])
        let harness = PresentationHarness(session: fixture.session)
        let totals = TaskTotalProbes(harness.dailyTotals, taskIDs: [firstTask.id])
        var observedErrors: [String?] = []
        var observedStatuses: [DailyTotalsStatus] = []
        var observedTexts: [String] = []
        var changedTotals: [Set<String>] = []
        for message in ["First report failure", "Second report failure"] {
            fixture.scheduler.poll?.fire()
            let report = try await fixture.client.next()
            report.fail(BridgeFailure(message: message))
            let finished = Task { @MainActor in
                while fixture.client.hasPendingRequests && !Task.isCancelled { await Task.yield() }
            }
            try await fixture.taskValue(finished)
            observedErrors.append(fixture.totalsError)
            observedStatuses.append(harness.dailyTotals.status)
            observedTexts.append(harness.dailyTotals.taskTotal(taskID: firstTask.id).content?.text ?? "")
            changedTotals.append(totals.changed())
        }
        XCTAssertEqual(observedErrors, ["First report failure", "Second report failure"])
        XCTAssertEqual(observedStatuses, [.cached, .cached])
        XCTAssertEqual(observedTexts, ["00:00:04", "00:00:04"])
        XCTAssertEqual(changedTotals, [Set([firstTask.id]), []], "Only the cached explanation changes the label.")
    }

    @MainActor
    func testSessionPublishesNewDayDuringEmptyStartupBeforeReportBecomesAvailable() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        let harness = PresentationHarness(session: fixture.session)
        var observedDays: [Date?] = []
        var observedStatuses: [DailyTotalsStatus] = []
        var observedErrors: [String?] = []
        var previous = (fixture.totalsDayStart, harness.dailyTotals.status)
        fixture.sync.onChange = {
            harness.update()
            let current = (fixture.totalsDayStart, harness.dailyTotals.status)
            guard current != previous else { return }
            previous = current
            observedDays.append(fixture.totalsDayStart)
            observedStatuses.append(harness.dailyTotals.status)
            observedErrors.append(fixture.totalsError)
        }
        fixture.sync.start()
        let open = try await fixture.client.next()
        fixture.clock.now.addTimeInterval(86_400)
        open.succeed(emptyTaskListResources)
        let report = try await fixture.client.next()
        XCTAssertEqual(try XCTUnwrap(observedDays.first), fixture.clock.now)
        XCTAssertEqual(
            observedStatuses.first, .unavailable,
            "The changed day must be published before loading changes its status.")
        XCTAssertNil(try XCTUnwrap(observedErrors.first))
        XCTAssertTrue(fixture.session.tasks.isEmpty)
        report.succeed(TaskListTotalsRefresh(taskList: emptyTaskListResources, rows: []))
        let finished = Task { @MainActor in
            while fixture.client.hasPendingRequests && !Task.isCancelled { await Task.yield() }
        }
        try await fixture.taskValue(finished)
        XCTAssertEqual(observedStatuses.last, .current)
        XCTAssertTrue(observedDays.allSatisfy { $0 == fixture.clock.now })
    }

    @MainActor
    func testDelayedReportFailureAcrossMidnightLoadsTodaysTotalsOnNextPoll() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        fixture.clock.now = dailyDate("2025-01-01T23:59:58.000Z")
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources, rows: [total(firstTask, 30)])
        fixture.sync.refresh()
        let yesterday = try await fixture.client.next()
        fixture.clock.now.addTimeInterval(2)
        yesterday.fail(BridgeFailure(message: "Yesterday's report failed"))
        try await fixture.settled()
        XCTAssertNil(fixture.session.dailyDuration(taskID: firstTask.id))
        XCTAssertEqual(fixture.totalsDayStart, fixture.clock.now, "The failure must move to today.")
        XCTAssertEqual(fixture.totalsStatus, .unavailable)
        XCTAssertEqual(fixture.totalsError, "Yesterday's report failed")
        fixture.scheduler.poll?.fire()
        let today = try await fixture.client.next()
        guard case .report(_, let start, let end, let now) = today.operation else {
            return XCTFail("The next poll after midnight must request today's report.")
        }
        XCTAssertEqual(start, "2025-01-02T00:00:00.000Z")
        XCTAssertEqual(end, "2025-01-03T00:00:00.000Z")
        XCTAssertEqual(now, "2025-01-02T00:00:00.000Z")
        XCTAssertEqual(fixture.totalsDayStart, fixture.clock.now)
        XCTAssertNil(fixture.session.dailyDuration(taskID: firstTask.id))
        XCTAssertTrue(fixture.todayTasks.isEmpty)
        XCTAssertFalse(fixture.session.isStale)
        today.succeed(TaskListTotalsRefresh(taskList: resources, rows: [total(firstTask, 2)]))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.dailyDuration(taskID: firstTask.id), 2)
        XCTAssertEqual(fixture.totalsStatus, .current)
        XCTAssertNil(fixture.totalsError)
    }

    @MainActor
    func testStaleRefreshClearsYesterdayDayAndErrorBeforeItsResponseArrives() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources, rows: [total(firstTask, 4)])
        fixture.scheduler.poll?.fire()
        let failed = try await fixture.client.next()
        failed.fail(BridgeFailure(message: "Yesterday unavailable", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertEqual(fixture.totalsError, "Yesterday unavailable")
        fixture.clock.now.addTimeInterval(86_400)
        fixture.sync.refresh()
        let refresh = try await fixture.client.next()
        XCTAssertEqual(fixture.totalsDayStart, fixture.clock.now)
        XCTAssertNil(fixture.totalsError)
        XCTAssertTrue(fixture.todayTasks.isEmpty)
        XCTAssertEqual(refresh.operation, .refresh(.local))
        refresh.succeed(resources)
        let report = try await fixture.client.next()
        report.succeed(TaskListTotalsRefresh(taskList: resources, rows: []))
        try await fixture.settled()
    }

    @MainActor
    func testTrackingReportAcrossMidnightPublishesTodaysDayAndClearsYesterdayError() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        fixture.clock.now = dailyDate("2025-01-01T23:59:59.000Z")
        let resources = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(resources, rows: [total(firstTask, 4)])
        fixture.scheduler.poll?.fire()
        let failed = try await fixture.client.next()
        failed.fail(BridgeFailure(message: "Yesterday totals unavailable"))
        try await fixture.settled()
        fixture.trackingActions.stopTracking(worklogID: activeWorklog.id)
        let stop = try await fixture.client.next()
        fixture.clock.now.addTimeInterval(1)
        var observedDays: [Date?] = []
        var observedErrors: [String?] = []
        fixture.sync.onChange = {
            if fixture.session.active == nil {
                observedDays.append(fixture.totalsDayStart)
                observedErrors.append(fixture.totalsError)
            }
        }
        let stoppedResources = TaskListResources(tasks: [firstTask], active: nil)
        stop.succeed(stoppedResources)
        let report = try await fixture.client.next()
        XCTAssertTrue(
            observedDays.allSatisfy { $0 == fixture.clock.now },
            "The acknowledged resources must publish today's day before its report arrives.")
        XCTAssertTrue(
            observedErrors.allSatisfy { $0 == nil },
            "Yesterday's report error must disappear when the new day is published.")
        report.succeed(TaskListTotalsRefresh(taskList: stoppedResources, rows: [total(firstTask, 5)]))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(observedDays.isEmpty)
        XCTAssertTrue(observedDays.allSatisfy { $0 == fixture.clock.now })
        XCTAssertTrue(observedErrors.allSatisfy { $0 == nil })
    }

    @MainActor
    func testHealthyReportAfterTimezoneChangeMovesRolloverToTheNewLocalMidnight() async throws {
        let originalZone = NSTimeZone.default
        defer { NSTimeZone.default = originalZone }
        NSTimeZone.default = TimeZone(identifier: "UTC")!
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        fixture.clock.now = dailyDate("2025-01-01T12:00:00.000Z")
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources, rows: [total(firstTask, 4)])
        let initialRollover = try XCTUnwrap(fixture.scheduler.active.first { !$0.repeating && $0.tolerance == 0 })
        XCTAssertEqual(initialRollover.delay, 12 * 3_600)
        NSTimeZone.default = TimeZone(identifier: "America/New_York")!
        fixture.sync.refresh()
        let changedZone = try await fixture.client.next()
        guard case .report(_, let start, let end, _) = changedZone.operation else {
            return XCTFail("The changed timezone must refresh the authoritative report.")
        }
        XCTAssertEqual(start, "2025-01-01T05:00:00.000Z")
        XCTAssertEqual(end, "2025-01-02T05:00:00.000Z")
        changedZone.succeed(TaskListTotalsRefresh(taskList: resources, rows: [total(firstTask, 6)]))
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
        nextDay.succeed(TaskListTotalsRefresh(taskList: resources, rows: []))
        try await fixture.settled()
        XCTAssertTrue(fixture.todayTasks.isEmpty)
    }

    @MainActor
    func testReportPublishesNewActiveTrackingTogetherWithItsMatchingTotals() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        let tasks = [firstTask, secondTask]
        try await fixture.start(TaskListResources(tasks: tasks, active: nil), rows: [total(firstTask, 10)])
        let newActive = WorklogItem(
            id: "new-active", taskId: secondTask.id,
            start: "2025-01-01T00:00:00.000Z", end: nil)
        var publishedTotals: [TimeInterval?] = []
        fixture.sync.onChange = {
            if fixture.session.active == newActive {
                publishedTotals.append(fixture.session.dailyDuration(taskID: secondTask.id))
            }
        }
        fixture.scheduler.poll?.fire()
        let report = try await fixture.client.next()
        report.succeed(
            TaskListTotalsRefresh(
                taskList: TaskListResources(tasks: tasks, active: newActive),
                rows: [total(firstTask, 10), total(secondTask, 77)]))
        let finished = Task { @MainActor in
            while fixture.client.hasPendingRequests && !Task.isCancelled { await Task.yield() }
        }
        try await fixture.taskValue(finished)
        XCTAssertFalse(publishedTotals.isEmpty)
        XCTAssertTrue(
            publishedTotals.allSatisfy { $0 == 77 },
            "Observers must receive the new active worklog and its report totals together.")
    }

    @MainActor
    func testHealthyPollRefreshesSelectedResourcesAndRetainsAllTaskTotalsWithoutHistoryPagination() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [secondTask, archivedTask, firstTask], active: activeWorklog)
        try await fixture.start(resources, rows: [total(firstTask, 20), total(archivedTask, 50)])
        XCTAssertEqual(fixture.todayTasks.map(\.id), [archivedTask.id, firstTask.id])
        XCTAssertEqual(fixture.durationText(taskID: firstTask.id), "00:00:20")
        XCTAssertEqual(fixture.session.dailyDuration(taskID: secondTask.id), 0)
        XCTAssertTrue(fixture.session.worklogHistory.worklogs.isEmpty)
        let count = fixture.client.operations.count
        fixture.scheduler.poll?.fire()
        let poll = try await fixture.client.next()
        guard case .report = poll.operation else { return XCTFail("Healthy polls must request the report directly.") }
        XCTAssertEqual(fixture.totalsStatus, .current)
        poll.succeed(TaskListTotalsRefresh(taskList: resources, rows: [total(firstTask, 25), total(archivedTask, 50)]))
        try await fixture.settled()
        XCTAssertEqual(fixture.client.operations.count, count + 1)
        XCTAssertEqual(fixture.session.dailyDuration(taskID: firstTask.id), 25)
    }

    @MainActor
    func testDisplayTicksUpdateOnlyTheRunningTaskTotalAndDoNotInvalidateTaskLists() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        fixture.sync.setWindowVisible(true)
        try await fixture.start(
            TaskListResources(tasks: [firstTask], active: activeWorklog), rows: [total(firstTask, 10)])
        let harness = PresentationHarness(session: fixture.session)
        let content = harness.counter(PresentationReads.content)
        let totals = TaskTotalProbes(harness.dailyTotals, taskIDs: [firstTask.id])
        let count = fixture.client.operations.count
        var changedTotals: [Set<String>] = []
        for _ in 0..<3 {
            fixture.clock.now.addTimeInterval(1)
            fixture.clock.uptime += 1
            fixture.scheduler.display?.fire()
            changedTotals.append(totals.changed())
        }
        XCTAssertEqual(changedTotals, Array(repeating: Set([firstTask.id]), count: 3))
        XCTAssertEqual(content.count, 0)
        XCTAssertEqual(fixture.client.operations.count, count)
        fixture.scheduler.display?.fire()
        XCTAssertEqual(totals.changed(), [])
    }

    @MainActor
    func testFailureRequiringRefreshDisablesWritesAndRecoversWithRefreshBeforeReport() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(resources, rows: [total(firstTask, 10)])
        fixture.scheduler.poll?.fire()
        let report = try await fixture.client.next()
        report.fail(BridgeFailure(message: "State changed", requiresRefresh: true))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.isStale)
        XCTAssertFalse(fixture.trackingActions.canStopTracking)
        XCTAssertEqual(fixture.totalsStatus, .cached)
        fixture.sync.refresh()
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .refresh(.local))
        refresh.succeed(resources)
        let replacement = try await fixture.client.next()
        guard case .report = replacement.operation else {
            return XCTFail("Recovery must reload totals before history.")
        }
        XCTAssertTrue(fixture.trackingActions.canStopTracking)
        replacement.succeed(TaskListTotalsRefresh(taskList: resources, rows: [total(firstTask, 15)]))
        try await fixture.settled()
        XCTAssertFalse(fixture.session.isStale)
        XCTAssertTrue(fixture.trackingActions.canStopTracking)
        XCTAssertEqual(fixture.totalsStatus, .current)
    }

    @MainActor
    func testUnavailableAndProtocolReportFailuresRequireResourceRecovery() async throws {
        let failures = [
            BridgeFailure(message: "Report offline", kind: "unavailable"),
            BridgeFailure(message: "Report incompatible", kind: "protocol"),
        ]
        for failure in failures {
            let fixture = Fixture(reports: true, calendar: dailyCalendar())
            defer { fixture.cleanup() }
            let resources = TaskListResources(tasks: [firstTask], active: nil)
            try await fixture.start(resources, rows: [total(firstTask, 4)])
            fixture.scheduler.poll?.fire()
            let failed = try await fixture.client.next()
            failed.fail(failure)
            try await fixture.settled()
            XCTAssertEqual(fixture.totalsStatus, .cached, failure.message)
            XCTAssertEqual(fixture.session.dailyDuration(taskID: firstTask.id), 4, failure.message)
            XCTAssertTrue(fixture.session.isStale, failure.message)
            XCTAssertFalse(fixture.trackingActions.canStartSelectedTask, failure.message)
            XCTAssertFalse(fixture.trackingActions.canStopTracking, failure.message)
            if failure.kind == "protocol" {
                XCTAssertNil(fixture.scheduler.poll)
                XCTAssertEqual(fixture.session.connectionStatusText, "Incompatible server")
            } else {
                XCTAssertNotNil(fixture.scheduler.poll)
            }
            fixture.sync.refresh()
            let refresh = try await fixture.client.next()
            XCTAssertEqual(refresh.operation, .refresh(.local), failure.message)
            refresh.succeed(resources)
            let report = try await fixture.client.next()
            guard case .report = report.operation else {
                return XCTFail("Resource recovery must precede the next report.")
            }
            XCTAssertTrue(fixture.trackingActions.canStartSelectedTask)
            report.succeed(TaskListTotalsRefresh(taskList: resources, rows: [total(firstTask, 8)]))
            try await fixture.settled()
            XCTAssertFalse(fixture.session.isStale, failure.message)
            XCTAssertTrue(fixture.trackingActions.canStartSelectedTask, failure.message)
            XCTAssertEqual(fixture.totalsStatus, .current, failure.message)
        }
    }

    @MainActor
    func testMalformedReportFailsAsProtocolBeforePublishingResources() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(resources, rows: [total(firstTask, 4)])
        fixture.scheduler.poll?.fire()
        let report = try await fixture.client.next()
        let replacementActive = WorklogItem(
            id: "replacement", taskId: secondTask.id,
            start: "2025-01-01T00:00:00.000Z", end: nil)
        report.succeed(
            TaskListTotalsRefresh(
                taskList: TaskListResources(tasks: [secondTask], active: replacementActive),
                rows: [total(firstTask, 8), total(firstTask, 2)]))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.tasks, [firstTask])
        XCTAssertEqual(fixture.session.active, activeWorklog)
        XCTAssertEqual(fixture.session.dailyDuration(taskID: firstTask.id), 4)
        XCTAssertEqual(fixture.totalsStatus, .cached)
        XCTAssertTrue(fixture.session.isStale)
        XCTAssertFalse(fixture.trackingActions.canStopTracking)
        XCTAssertEqual(fixture.session.connectionStatusText, "Incompatible server")
        XCTAssertNil(fixture.scheduler.poll)
    }

    @MainActor
    func testTaskListRefreshAfterStaleConnectionMarksTotalsCachedUntilTheReportArrives() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(resources, rows: [total(firstTask, 10)])
        XCTAssertEqual(fixture.totalsStatus, .current)
        fixture.sync.retryHistory()
        let history = try await fixture.client.next()
        history.fail(BridgeFailure(message: "History unavailable", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.isStale)
        XCTAssertEqual(fixture.totalsStatus, .current)

        fixture.sync.refresh()
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .refresh(.local))
        refresh.succeed(resources)
        let report = try await fixture.client.next()
        guard case .report = report.operation else {
            return XCTFail("The refreshed task list must reload totals, got \(report.operation).")
        }
        XCTAssertEqual(fixture.totalsStatus, .cached)
        report.succeed(TaskListTotalsRefresh(taskList: resources, rows: [total(firstTask, 15)]))
        try await fixture.client.next().succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.totalsStatus, .current)
    }

    @MainActor
    func testBothLocalDayTimersClearYesterdayWithoutRetryingProtocolFailure() async throws {
        for useDisplayTimer in [false, true] {
            let fixture = Fixture(reports: true, calendar: dailyCalendar())
            defer { fixture.cleanup() }
            fixture.sync.setWindowVisible(true)
            let resources = TaskListResources(tasks: [firstTask], active: activeWorklog)
            try await fixture.start(resources, rows: [total(firstTask, 4)])
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
            XCTAssertEqual(fixture.totalsDayStart, fixture.clock.now)
            XCTAssertEqual(fixture.client.operations.count, operationCount)
            XCTAssertFalse(fixture.client.hasPendingRequests)
            XCTAssertNil(fixture.session.dailyDuration(taskID: firstTask.id))
            XCTAssertTrue(fixture.todayTasks.isEmpty)
            XCTAssertEqual(fixture.totalsStatus, .unavailable)
            XCTAssertEqual(fixture.session.connectionStatusText, "Incompatible server")
            XCTAssertNil(fixture.scheduler.poll)
        }
    }

    @MainActor
    func testSuccessfulSourceChangeClearsTotalsButFailedCandidatePreservesThem() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources, rows: [total(firstTask, 100)])
        let failed = Task { await fixture.sync.connect(serverSettings) }
        let failure = try await fixture.client.next()
        failure.fail(BridgeFailure(message: "Candidate offline"))
        let failedResult = try await fixture.taskValue(failed)
        XCTAssertFalse(failedResult)
        XCTAssertEqual(fixture.session.dailyDuration(taskID: firstTask.id), 100)
        let connected = Task { await fixture.sync.connect(serverSettings) }
        let connect = try await fixture.client.next()
        connect.succeed(resources)
        let report = try await fixture.client.next()
        XCTAssertNil(fixture.session.dailyDuration(taskID: firstTask.id))
        guard case .report(let settings, _, _, _) = report.operation else {
            return XCTFail("New source must load its own report.")
        }
        XCTAssertEqual(settings, serverSettings)
        report.succeed(TaskListTotalsRefresh(taskList: resources, rows: []))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        let connectedResult = try await fixture.taskValue(connected)
        XCTAssertTrue(connectedResult)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.dailyDuration(taskID: firstTask.id), 0)
    }

    @MainActor
    func testTrackingCommandReloadsReportBeforeHistoryAndNeverAddsWholeActiveElapsed() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(resources, rows: [total(firstTask, 5)])
        fixture.trackingActions.stopTracking(worklogID: activeWorklog.id)
        let stop = try await fixture.client.next()
        stop.succeed(TaskListResources(tasks: [firstTask], active: nil))
        let report = try await fixture.client.next()
        guard case .report = report.operation else { return XCTFail("Tracking changes must report before history.") }
        XCTAssertEqual(fixture.totalsStatus, .current)
        XCTAssertEqual(fixture.session.active, activeWorklog)
        report.succeed(
            TaskListTotalsRefresh(
                taskList: TaskListResources(tasks: [firstTask], active: nil), rows: [total(firstTask, 7)]))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.dailyDuration(taskID: firstTask.id), 7)
    }

    @MainActor
    func testIdleMidnightTimerClearsYesterdayAndRequestsTodaysReport() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        fixture.clock.now = dailyDate("2025-01-01T23:59:58.000Z")
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources, rows: [total(firstTask, 30)])
        XCTAssertNil(fixture.scheduler.display)
        let rollover = try XCTUnwrap(fixture.scheduler.active.first { !$0.repeating && $0.delay == 2 })
        fixture.clock.now.addTimeInterval(2)
        rollover.fire()
        let report = try await fixture.client.next()
        guard case .report(_, let start, _, _) = report.operation else {
            return XCTFail("Idle midnight must fetch the new day.")
        }
        XCTAssertEqual(start, "2025-01-02T00:00:00.000Z")
        XCTAssertTrue(fixture.todayTasks.isEmpty)
        report.succeed(TaskListTotalsRefresh(taskList: resources, rows: []))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.dailyDuration(taskID: firstTask.id), 0)
    }

    @MainActor
    func testDelayedReportAcrossMidnightCannotRestoreYesterdayAndNextPollLoadsToday() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        fixture.clock.now = dailyDate("2025-01-01T23:59:58.000Z")
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources, rows: [total(firstTask, 30)])
        let rollover = try XCTUnwrap(fixture.scheduler.active.first { !$0.repeating && $0.delay == 2 })
        fixture.sync.refresh()
        let yesterday = try await fixture.client.next()
        fixture.clock.now.addTimeInterval(2)
        rollover.fire()
        XCTAssertNil(fixture.session.dailyDuration(taskID: firstTask.id))
        yesterday.succeed(TaskListTotalsRefresh(taskList: resources, rows: [total(firstTask, 40)]))
        try await fixture.settled()
        XCTAssertNil(fixture.session.dailyDuration(taskID: firstTask.id))
        fixture.scheduler.poll?.fire()
        let today = try await fixture.client.next()
        guard case .report(_, let start, _, _) = today.operation else {
            return XCTFail("Delayed report must be replaced for today.")
        }
        XCTAssertEqual(start, "2025-01-02T00:00:00.000Z")
        XCTAssertNil(fixture.session.dailyDuration(taskID: firstTask.id))
        today.succeed(TaskListTotalsRefresh(taskList: resources, rows: [total(firstTask, 2)]))
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
        fixture.sync.setWindowVisible(true)
        let resources = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(resources, rows: [total(firstTask, 4)])
        let timers = fixture.scheduler.active
        fixture.sync.sleep()
        XCTAssertTrue(timers.allSatisfy(\.cancelled))
        XCTAssertTrue(fixture.scheduler.active.isEmpty)
        fixture.clock.now.addTimeInterval(300)
        fixture.sync.wake()
        let wake = try await fixture.client.next()
        guard case .report = wake.operation else {
            return XCTFail("Healthy wake must refresh the authoritative report.")
        }
        XCTAssertEqual(fixture.session.dailyDuration(taskID: firstTask.id), 304)
        wake.succeed(TaskListTotalsRefresh(taskList: resources, rows: [total(firstTask, 304)]))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.dailyDuration(taskID: firstTask.id), 304)
    }

    @MainActor
    func testUnchangedHealthyPollDoesNotNotifyContentOrDailyTotalsAndFailureNotifiesStatus() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources, rows: [total(firstTask, 4)])
        let harness = PresentationHarness(session: fixture.session)
        let content = harness.counter(PresentationReads.content)
        let totals = TaskTotalProbes(harness.dailyTotals, taskIDs: [firstTask.id])
        fixture.scheduler.poll?.fire()
        let poll = try await fixture.client.next()
        poll.succeed(TaskListTotalsRefresh(taskList: resources, rows: [total(firstTask, 4)]))
        let finished = Task { @MainActor in
            while fixture.client.hasPendingRequests && !Task.isCancelled { await Task.yield() }
        }
        try await fixture.taskValue(finished)
        XCTAssertEqual(content.count, 0)
        XCTAssertEqual(totals.changed(), [])
        fixture.scheduler.poll?.fire()
        let failed = try await fixture.client.next()
        failed.fail(BridgeFailure(message: "Totals unavailable"))
        let failureFinished = Task { @MainActor in
            while fixture.client.hasPendingRequests && !Task.isCancelled { await Task.yield() }
        }
        try await fixture.taskValue(failureFinished)
        XCTAssertEqual(content.count, 1)
        XCTAssertEqual(totals.changed(), Set([firstTask.id]))
        XCTAssertEqual(fixture.session.dailyDuration(taskID: firstTask.id), 4)
        XCTAssertFalse(fixture.session.isStale)
        XCTAssertTrue(fixture.trackingActions.canStartSelectedTask)
    }

    @MainActor
    func testStatusReadersAreNotifiedOnlyWhenTheShownStatusChanges() async throws {
        let fixture = Fixture(reports: true, calendar: dailyCalendar())
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        let rows = [total(firstTask, 10)]
        try await fixture.start(resources, rows: rows)
        XCTAssertEqual(fixture.session.dailyTotals.status, .current)
        let status = ObservationProbe { _ = fixture.session.dailyTotals.status }

        fixture.scheduler.poll?.fire()
        try await fixture.client.next().succeed(TaskListTotalsRefresh(taskList: resources, rows: rows))
        try await fixture.settled()
        XCTAssertFalse(status.consume(), "An unchanged report must not notify status readers.")

        fixture.scheduler.poll?.fire()
        try await fixture.client.next().fail(BridgeFailure(message: "Report failed"))
        try await fixture.settled()
        XCTAssertTrue(status.consume())
        XCTAssertEqual(fixture.session.dailyTotals.status, .cached)
        XCTAssertEqual(
            fixture.session.dailyTotals.taskTotal(taskID: firstTask.id).content?.explanation,
            "Today's total uses cached tracker state. Running time may be unconfirmed.")
    }
}

/// Daily totals computed from the clock on each read, so a test can check them without a publish.
private extension Fixture {
    var totalsStatus: DailyTotalsStatus { session.dailyTotals.shownStatus(clock: clock) }
    var totalsError: String? { session.dailyTotals.error }
    var totalsDayStart: Date? { session.dailyTotals.day?.start }
    var totalDuration: TimeInterval? { session.dailyTotals.totalDuration(active: session.active, clock: clock) }
    var totalDurationText: String { totalDuration.map(formatClockDuration) ?? "Unavailable" }
    /// Tasks with time logged today, in task list order.
    var todayTasks: [TaskItem] { session.tasks.filter { (session.dailyDuration(taskID: $0.id) ?? 0) > 0 } }

    func durationText(taskID: String) -> String {
        session.dailyDuration(taskID: taskID).map(formatClockDuration) ?? "Unavailable"
    }
}
