import XCTest

/// macOS shows one sheet per window. The task and worklog menus have no check of their own, so these tests show that
/// an attached sheet or alert blocks them.
@MainActor
final class OpenDialogUITests: TrackerUITestCase {
    // The alert needs a rejected tracking command, and only the proxy can hold that command for a competing client.
    override var fixtureSource: TrackerFixture.Source { .server }

    private struct MenuEntry {
        let name: String
        let target: XCUICoordinate
        let open: (XCUICoordinate) -> Void
        let item: String
    }

    func testOpenSheetBlocksTaskAndWorklogMenus() throws {
        let (task, log) = try seedTaskWithCompletedWorklog()
        launch()
        select(task)
        let entries = menuEntries(task: task, log: log)
        assertMenusOpenWithoutDialog(entries)

        openCreation()
        XCTAssertEqual(openDialogCount, 1)
        for entry in entries {
            attemptToOpen(entry)
            XCTAssertTrue(element("task-name.input").exists, "\(entry.name) replaced the open sheet.")
        }

        app.buttons["task-name.cancel"].click()
        assertNoDialogRemains()
    }

    func testTrackingFailureAlertBlocksTaskAndWorklogMenus() throws {
        let (task, log) = try seedTaskWithCompletedWorklog()
        let competing = try fixture.create("Competing timer")
        let proxy = try fixture.enableProxy()
        launch()
        select(task)
        let entries = menuEntries(task: task, log: log)
        assertMenusOpenWithoutDialog(entries)

        try proxy.arm([
            .init(method: "GET", path: "/v1/state/daily-overview", mode: .holdAfter),
            .init(method: "PUT", path: "/v1/tracking", mode: .holdBefore),
        ])
        _ = try proxy.waitForHeldRequest()
        app.buttons["Start tracking"].click()
        try proxy.release()
        // Hold the start before the server sees it while the competing client starts a timer.
        _ = try proxy.waitForHeldRequest()
        _ = try fixture.start(competing)
        try proxy.release()

        let title = app.staticTexts["Could not change tracking"]
        waitUntil("The tracking failure alert appears") { title.exists }
        XCTAssertEqual(openDialogCount, 1)
        for entry in entries {
            attemptToOpen(entry)
            XCTAssertTrue(title.exists, "\(entry.name) replaced the open alert.")
            XCTAssertFalse(element("task-name.input").exists, "\(entry.name) opened the rename editor.")
        }

        dismissTrackingFailure(containing: "changed")
        assertNoDialogRemains()
    }

    private func seedTaskWithCompletedWorklog() throws -> (FixtureTask, FixtureWorklog) {
        let task = try fixture.create("Menu target")
        let log = try fixture.completed(
            task, start: Date().addingTimeInterval(-3600), end: Date().addingTimeInterval(-1800))
        return (task, log)
    }

    /// The worklog targets sit at the row's trailing end, because a sheet opens over the middle of the window.
    private func menuEntries(task: FixtureTask, log: FixtureWorklog) -> [MenuEntry] {
        let center = CGVector(dx: 0.5, dy: 0.5)
        let row = element("worklog-history.row.\(log.id)")
        XCTAssertTrue(row.waitForExistence(timeout: timeout))
        return [
            MenuEntry(
                name: "The task context menu", target: taskRow(task).coordinate(withNormalizedOffset: center),
                open: { $0.rightClick() }, item: "Edit name"),
            MenuEntry(
                name: "The worklog context menu",
                target: row.coordinate(withNormalizedOffset: CGVector(dx: 0.9, dy: 0.5)),
                open: { $0.rightClick() }, item: "Edit times..."),
            MenuEntry(
                name: "The worklog actions menu",
                target: element("worklog-history.actions.\(log.id)").coordinate(withNormalizedOffset: center),
                open: { $0.click() }, item: "Edit times..."),
        ]
    }

    /// The same clicks must open each menu while no dialog shows, or the blocked attempts prove nothing.
    private func assertMenusOpenWithoutDialog(_ entries: [MenuEntry]) {
        XCTAssertEqual(openDialogCount, 0)
        for entry in entries {
            entry.open(entry.target)
            let item = app.menuItems[entry.item]
            XCTAssertTrue(item.waitForExistence(timeout: timeout), "\(entry.name) did not open.")
            app.typeKey(.escape, modifierFlags: [])
            waitUntil("\(entry.name) closes") { !item.exists }
        }
    }

    /// Clicks through coordinates, so XCTest sends the mouse event to the blocked window as a person's click would.
    private func attemptToOpen(_ entry: MenuEntry) {
        XCTAssertFalse(
            openDialogElements.contains { $0.frame.contains(entry.target.screenPoint) },
            "\(entry.name) lies under the dialog, so this click cannot reach it.")
        entry.open(entry.target)
        let opened = app.menuItems[entry.item].waitForExistence(timeout: 2)
        XCTAssertFalse(opened, "\(entry.name) opened behind the dialog.")
        XCTAssertEqual(openDialogCount, 1, "\(entry.name) opened a second dialog.")
    }

    /// An editor that opened behind the first dialog would show now. One that never shows would keep the flag set and
    /// leave New task disabled.
    private func assertNoDialogRemains() {
        let newTask = app.buttons["New task"]
        waitUntil("All dialogs close and New task returns") { self.openDialogCount == 0 && newTask.isEnabled }
    }

    /// SwiftUI may report an alert as either element type.
    private var openDialogElements: [XCUIElement] {
        app.sheets.allElementsBoundByIndex + app.dialogs.allElementsBoundByIndex
    }

    private var openDialogCount: Int { app.sheets.count + app.dialogs.count }
}
