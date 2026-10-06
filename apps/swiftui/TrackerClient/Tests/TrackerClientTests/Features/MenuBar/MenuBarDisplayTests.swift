import Foundation
import XCTest
@testable import TrackerClient

final class MenuBarDisplayTests: XCTestCase {
    @MainActor
    func testPreferencesDefaultAndMigrateBothLegacyValuesWithoutWriting() {
        let suite = "MenuBarDisplayTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let preferences = UserDefaultsMenuBarPreferences(defaults: defaults)
        XCTAssertEqual(preferences.load(), .time)
        for legacy in [false, true] {
            defaults.set(legacy, forKey: "tracker.showDailyTotalInMenuBar")
            XCTAssertEqual(preferences.load(), legacy ? .time : .none)
            XCTAssertNil(defaults.object(forKey: "tracker.menuBarDisplay"))
        }
        defaults.set("invalid", forKey: "tracker.showDailyTotalInMenuBar")
        XCTAssertEqual(preferences.load(), .time)
    }

    @MainActor
    func testPreferencesPersistEverySelectionAndOverrideLegacyWithoutSavingTaskNames() {
        let suite = "MenuBarDisplayTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let preferences = UserDefaultsMenuBarPreferences(defaults: defaults)
        for display in MenuBarDisplay.allCases {
            defaults.set(display == .none, forKey: "tracker.showDailyTotalInMenuBar")
            preferences.save(display)
            XCTAssertEqual(defaults.string(forKey: "tracker.menuBarDisplay"), display.rawValue)
            XCTAssertEqual(UserDefaultsMenuBarPreferences(defaults: defaults).load(), display)
        }
        XCTAssertEqual(defaults.persistentDomain(forName: suite)?.count, 2)
    }

    @MainActor
    func testUnknownAndMalformedPreferencesFallBackToLegacyOrDefault() {
        let suite = "MenuBarDisplayTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let preferences = UserDefaultsMenuBarPreferences(defaults: defaults)
        for invalid in ["future-display", "", true, 42] as [Any] {
            defaults.set(invalid, forKey: "tracker.menuBarDisplay")
            defaults.removeObject(forKey: "tracker.showDailyTotalInMenuBar")
            XCTAssertEqual(preferences.load(), .time)
            defaults.set(false, forKey: "tracker.showDailyTotalInMenuBar")
            XCTAssertEqual(preferences.load(), .none)
            defaults.set(true, forKey: "tracker.showDailyTotalInMenuBar")
            XCTAssertEqual(preferences.load(), .time)
        }
    }

    @MainActor
    func testModesSwitchPreserveTrackingStateAndKeepTheFullTaskTooltip() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask], active: activeWorklog),
                                rows: [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 90_000_000)])
        let observer = TrackerMenuPresentationObserver(session: fixture.session, display: .time)
        let content = observer.content
        var contentChanges = 0
        var labelChanges = 0
        observer.onContentChange = { contentChanges += 1 }
        observer.onLabelChange = { labelChanges += 1 }
        XCTAssertEqual(observer.label.text, "00:01")
        observer.update(from: fixture.session, display: .taskName)
        XCTAssertEqual(observer.label.display, .taskName)
        XCTAssertEqual(observer.label.text, firstTask.name)
        XCTAssertEqual(observer.label.help, "Tracking: First task")
        fixture.clock.now.addTimeInterval(180)
        fixture.clock.uptime += 180
        observer.update(from: fixture.session, display: .taskName)
        XCTAssertEqual(labelChanges, 1, "Task labels must not change when only elapsed time advances.")
        observer.update(from: fixture.session, display: .none)
        XCTAssertNil(observer.label.text)
        XCTAssertEqual(observer.label.help, "Tracking: First task")
        observer.update(from: fixture.session, display: .time)
        XCTAssertEqual(observer.label.text, "00:04")
        XCTAssertEqual(labelChanges, 3)
        XCTAssertEqual(contentChanges, 1, "The clock changes menu totals independently of the selected label.")
        XCTAssertEqual(observer.content.activeWorklogID, content.activeWorklogID)
    }

    @MainActor
    func testTaskLabelsRespectTheBoundaryAndKeepCombinedCharactersWhole() async throws {
        let cases = [
            (String(repeating: "a", count: 23), String(repeating: "a", count: 23)),
            (String(repeating: "a", count: 24), String(repeating: "a", count: 24)),
            (String(repeating: "a", count: 25), "aaaaaaaaaaaaaaaaaaaaaaa…"),
            (String(repeating: "👩🏽‍💻", count: 25), String(repeating: "👩🏽‍💻", count: 23) + "…"),
            (String(repeating: "e\u{301}", count: 25), String(repeating: "e\u{301}", count: 23) + "…")
        ]
        for (name, expected) in cases {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            let task = TaskItem(id: firstTask.id, name: name, archived: false, latestStart: nil)
            try await fixture.start(TrackerSnapshot(tasks: [task], active: activeWorklog))
            let label = TrackerMenuLabelContent(fixture.session, display: .taskName)
            XCTAssertEqual(label.text, expected)
            XCTAssertEqual(label.text?.count, min(name.count, 24))
            XCTAssertEqual(label.help, "Tracking: \(name)")
        }
    }

    @MainActor
    func testTaskLabelFollowsRenamesSwitchesAndStoppingFromConfirmedSnapshots() async throws {
        let fixture = Fixture(saved: serverSettings, reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog))
        let observer = TrackerMenuPresentationObserver(session: fixture.session, display: .taskName)
        XCTAssertEqual(observer.label.text, firstTask.name)
        let renamed = TaskItem(id: firstTask.id, name: "Renamed active task", archived: false, latestStart: nil)
        let switched = WorklogItem(id: "switched-worklog", taskId: secondTask.id, start: activeWorklog.start, end: nil)
        let snapshots = [TrackerSnapshot(tasks: [renamed, secondTask], active: activeWorklog),
                         TrackerSnapshot(tasks: [renamed, secondTask], active: switched),
                         TrackerSnapshot(tasks: [renamed, secondTask], active: nil)]
        for (index, snapshot) in snapshots.enumerated() {
            fixture.scheduler.poll?.fire()
            let poll = try await fixture.client.next()
            poll.succeed(TrackerReport(snapshot: snapshot, rows: []))
            if index == 1 {
                let history = try await fixture.client.next()
                history.succeed(emptyPage)
            }
            try await fixture.settled()
            observer.update(from: fixture.session, display: .taskName)
            if snapshot.active?.taskId == firstTask.id {
                XCTAssertEqual(observer.label.text, renamed.name)
                XCTAssertEqual(observer.label.help, "Tracking: Renamed active task")
            } else if snapshot.active != nil {
                XCTAssertEqual(observer.label.text, secondTask.name)
                XCTAssertEqual(observer.label.help, "Tracking: Second task")
            } else {
                XCTAssertNil(observer.label.text)
                XCTAssertEqual(observer.label.help, "Stopped. Last tracked: Second task")
            }
        }
    }

    @MainActor
    func testStaleTaskLabelKeepsTheConfirmedNameAndOutlinesItsIndicator() async throws {
        let fixture = Fixture(saved: serverSettings, reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask], active: activeWorklog))
        fixture.scheduler.poll?.fire()
        let poll = try await fixture.client.next()
        poll.fail(BridgeFailure(message: "Server unavailable", kind: "unavailable"))
        try await fixture.settled()
        let label = TrackerMenuLabelContent(fixture.session, display: .taskName)
        XCTAssertEqual(label.text, firstTask.name)
        XCTAssertEqual(label.symbol, "circle")
        XCTAssertEqual(label.help, "Tracking status unavailable. Last confirmed task: First task")
    }

    @MainActor
    func testIdleAndUnknownTasksNeverShowPlaceholderNamesInTheBar() async throws {
        for active in [nil, activeWorklog] as [WorklogItem?] {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            try await fixture.start(TrackerSnapshot(tasks: [], active: active))
            let label = TrackerMenuLabelContent(fixture.session, display: .taskName)
            XCTAssertNil(label.text)
            XCTAssertEqual(label.help, active == nil ? "No task tracked yet" : "Tracking: Unknown task")
        }
    }

    @MainActor
    func testDisplayAndRenamesStayFrozenUntilTheRootMenuCloses() async throws {
        let fixture = Fixture(saved: serverSettings, reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask], active: activeWorklog))
        let observer = TrackerMenuPresentationObserver(session: fixture.session, display: .taskName)
        observer.menuOpened(from: fixture.session, display: .taskName)
        observer.menuOpened(from: fixture.session, display: .none)
        let original = observer.label
        let renamed = TaskItem(id: firstTask.id, name: "Renamed while menu is open", archived: false, latestStart: nil)
        fixture.scheduler.poll?.fire()
        let poll = try await fixture.client.next()
        poll.succeed(TrackerReport(snapshot: TrackerSnapshot(tasks: [renamed], active: activeWorklog), rows: []))
        try await fixture.settled()
        observer.update(from: fixture.session, display: .none)
        observer.menuClosed(from: fixture.session, display: .none)
        XCTAssertEqual(observer.label, original)
        observer.menuClosed(from: fixture.session, display: .none)
        XCTAssertEqual(observer.label.display, .none)
        XCTAssertNil(observer.label.text)
        XCTAssertEqual(observer.label.help, "Tracking: Renamed while menu is open")
        observer.update(from: fixture.session, display: .taskName)
        XCTAssertEqual(observer.label.text, "Renamed while menu is o…")
    }
}
