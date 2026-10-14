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
        let environment = ProcessInfo.processInfo.environment
        let bindingName = environment["TT_UI_HISTORY_BINDING"] ?? environment["TEST_RUNNER_TT_UI_HISTORY_BINDING"] ?? "index"
        let binding = try XCTUnwrap(
            HistoryRowBinding(rawValue: bindingName),
            "Unsupported history binding '\(bindingName)'. Use 'index' or 'accessibility'.")
        let task = try fixture.create("History pagination")
        let now = Date()
        var logs: [FixtureWorklog] = []
        for index in 0..<53 {
            let start = now.addingTimeInterval(-Double(54 - index) * 600)
            logs.append(try fixture.completed(task, start: start, end: start.addingTimeInterval(300)))
        }
        launch()
        select(task)
        let expected = logs.reversed().map(\.id)
        let observed = try measureUIPhase("traversal") {
            let pagination = app.buttons["worklog-history.load-older"]
            scrollHistoryUntilVisible(pagination)
            pagination.click()
            waitUntil("Older page finishes") { !pagination.exists }
            scrollHistoryUntilVisible(element("worklog-history.row.\(expected[0])"), upward: true)
            let scroll = element("worklog-history.scroll")
            var observed: [String] = []
            for _ in 0..<50 {
                let visible = try visibleHistoryRows(in: scroll, binding: binding)
                let identifiers = visible.map { String($0.identifier.dropFirst("worklog-history.row.".count)) }
                XCTAssertEqual(identifiers.count, Set(identifiers).count, "Duplicate history rows in the visible page.")
                for identifier in identifiers where !observed.contains(identifier) { observed.append(identifier) }
                if identifiers.contains(expected.last!) { break }
                scroll.scroll(byDeltaX: 0, deltaY: -350)
            }
            return observed
        }
        try measureUIPhase("assertions") {
            XCTAssertEqual(observed, expected, "History must contain every expected ID exactly once, newest first.")
            let persisted = try fixture.worklogs(task)
            XCTAssertEqual(Set(persisted.map(\.id)), Set(logs.map(\.id)))
        }
        measureUIPhase("query-diagnostics") {
            print("TT_UI_HISTORY_TYPES row=\(historyRows.firstMatch.elementType.rawValue) pane=\(element("worklog-history.scroll").elementType.rawValue)")
            print("TT_UI_HISTORY_BINDING binding=\(binding.rawValue)")
        }
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

    private enum HistoryRowBinding: String {
        case index
        case accessibility

        func elements(in query: XCUIElementQuery) -> [XCUIElement] {
            switch self {
            case .index:
                return query.allElementsBoundByIndex
            case .accessibility:
                return query.allElementsBoundByAccessibilityElement
            }
        }
    }

    private struct HistoryRowSnapshot {
        let identifier: String
        let frame: CGRect
        let elementType: XCUIElement.ElementType

        init(_ snapshot: any XCUIElementSnapshot) {
            identifier = snapshot.identifier
            frame = snapshot.frame
            elementType = snapshot.elementType
        }
    }

    private func historyRowSnapshots(in snapshot: any XCUIElementSnapshot) -> [HistoryRowSnapshot] {
        var rows: [HistoryRowSnapshot] = []
        if snapshot.identifier.hasPrefix("worklog-history.row.") {
            rows.append(HistoryRowSnapshot(snapshot))
        }
        for child in snapshot.children {
            rows.append(contentsOf: historyRowSnapshots(in: child))
        }
        return rows
    }

    private func visibleHistoryRows(in scroll: XCUIElement, binding: HistoryRowBinding) throws -> [HistoryRowSnapshot] {
        let pane = try scroll.snapshot()
        let candidates = historyRowSnapshots(in: pane).filter { $0.frame.intersects(pane.frame) }
        var visible: [HistoryRowSnapshot] = []
        for elementType in Set(candidates.map(\.elementType)) {
            let identifiers = candidates.filter { $0.elementType == elementType }.map(\.identifier)
            let query = scroll.descendants(matching: elementType).matching(NSPredicate(format: "identifier IN %@", identifiers))
            for row in binding.elements(in: query) where row.isHittable {
                visible.append(HistoryRowSnapshot(try row.snapshot()))
            }
        }
        return visible.sorted { $0.frame.minY < $1.frame.minY }
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

    func testHeldSelectionHistoryShowsLoadingUntilEmptyResponseArrives() throws {
        let populated = try fixture.create("Populated history")
        let empty = try fixture.create("Empty destination")
        let log = try fixture.completed(populated, start: Date().addingTimeInterval(-3600), end: Date().addingTimeInterval(-1800))
        let proxy = try fixture.enableProxy()
        launch()
        select(populated)
        XCTAssertTrue(element("worklog-history.row.\(log.id)").waitForExistence(timeout: timeout))
        try proxy.arm(method: "GET", path: "/v1/worklogs?task_id=\(empty.id)", mode: .holdAfter)
        select(empty)
        _ = try proxy.waitForHeldRequest()
        let progress = app.activityIndicators["worklog-history.loading"]
        XCTAssertTrue(progress.waitForExistence(timeout: timeout))
        XCTAssertFalse(element("worklog-history.empty").exists)
        XCTAssertFalse(element("worklog-history.row.\(log.id)").exists)
        try proxy.release()
        XCTAssertTrue(element("worklog-history.empty").waitForExistence(timeout: timeout))
        XCTAssertFalse(progress.exists)
    }
}
