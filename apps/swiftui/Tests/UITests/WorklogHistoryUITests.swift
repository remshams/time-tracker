import XCTest

class WorklogHistoryUITests: TrackerUITestCase {
    func testHistoryShowsNewestFirstAndOnlySelectedTaskRows() throws {
        let first = try fixture.create("Implementation")
        let second = try fixture.create("Review")
        let now = Date()
        let older = try fixture.completed(first, start: now.addingTimeInterval(-10800), end: now.addingTimeInterval(-9000))
        let newer = try fixture.completed(first, start: now.addingTimeInterval(-7200), end: now.addingTimeInterval(-5400))
        let other = try fixture.completed(second, start: now.addingTimeInterval(-3600), end: now.addingTimeInterval(-1800))
        launch()
        select(first)
        let oldRow = element("worklog-history.row.\(older.id)")
        let newRow = element("worklog-history.row.\(newer.id)")
        XCTAssertTrue(oldRow.waitForExistence(timeout: timeout))
        XCTAssertTrue(newRow.exists)
        XCTAssertLessThan(newRow.frame.minY, oldRow.frame.minY)
        XCTAssertFalse(element("worklog-history.row.\(other.id)").exists)
        select(second)
        XCTAssertTrue(element("worklog-history.row.\(other.id)").waitForExistence(timeout: timeout))
        XCTAssertFalse(newRow.exists)
        XCTAssertFalse(oldRow.exists)
    }

    func testHistoryPaginationLoadsMoreThanFiftyWithoutMissingOrDuplicateIDs() throws {
        let task = try fixture.create("History pagination")
        let now = Date()
        var logs: [FixtureWorklog] = []
        for index in 0..<53 {
            let start = now.addingTimeInterval(-Double(54 - index) * 600)
            logs.append(try fixture.completed(task, start: start, end: start.addingTimeInterval(300)))
        }
        launch()
        select(task)
        let pagination = app.buttons["worklog-history.load-older"]
        scrollHistoryUntilVisible(pagination)
        pagination.click()
        waitUntil("Older page finishes") { !pagination.exists }
        let expected = logs.reversed().map(\.id)
        scrollHistoryUntilVisible(element("worklog-history.row.\(expected[0])"), upward: true)
        let scroll = element("worklog-history.scroll")
        var observed: [String] = []
        for _ in 0..<50 {
            let visible = historyRows.allElementsBoundByIndex.filter(\.isHittable).sorted { $0.frame.minY < $1.frame.minY }
            let identifiers = visible.map { String($0.identifier.dropFirst("worklog-history.row.".count)) }
            XCTAssertEqual(identifiers.count, Set(identifiers).count, "Duplicate history rows in the visible page.")
            for identifier in identifiers where !observed.contains(identifier) { observed.append(identifier) }
            if element("worklog-history.row.\(expected.last!)").isHittable { break }
            scroll.scroll(byDeltaX: 0, deltaY: -350)
        }
        XCTAssertEqual(observed, expected, "History must contain every expected ID exactly once, newest first.")
        XCTAssertEqual(Set(try fixture.worklogs(task).map(\.id)), Set(logs.map(\.id)))
    }

    func testEmptyHistoryAndRunningWorklogHaveExplicitStates() throws {
        let empty = try fixture.create("Empty history")
        let running = try fixture.create("Running history")
        let log = try fixture.start(running)
        launch()
        select(empty)
        XCTAssertTrue(element("worklog-history.empty").waitForExistence(timeout: timeout))
        select(running)
        XCTAssertTrue(element("worklog-history.row.\(log.id)").waitForExistence(timeout: timeout))
        XCTAssertFalse(element("worklog-history.empty").exists)
        XCTAssertTrue(element("worklog-history.row.\(log.id)").staticTexts.matching(NSPredicate(format: "value CONTAINS 'Running' OR label CONTAINS 'Running'")).firstMatch.exists)
    }

    private func scrollHistoryUntilVisible(_ target: XCUIElement, upward: Bool = false) {
        let scroll = element("worklog-history.scroll")
        for _ in 0..<30 {
            if target.exists && target.isHittable { return }
            scroll.hover()
            scroll.scroll(byDeltaX: 0, deltaY: upward ? 450 : -450)
        }
        XCTFail("History traversal could not reach \(target.identifier).")
    }
}

final class ServerWorklogHistoryUITests: WorklogHistoryUITests {
    override var fixtureSource: TrackerFixture.Source { .server }
}
