import XCTest

/// macOS shows one sheet per window. The context menus and Start tracking open without an open-dialog check, so these
/// tests show that an attached sheet or alert blocks them and keeps the window open.
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

    /// Start tracking checks only whether tracking can start, so only the window can block it.
    func testOpenSheetBlocksToolbarButtons() throws {
        let task = try fixture.create("Toolbar target")
        launch()
        select(task)
        let start = app.buttons["Start tracking"]
        waitUntil("Start tracking becomes available") { start.exists && start.isEnabled }
        let target = start.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))

        openCreation()
        XCTAssertFalse(
            openDialogElements.contains { $0.frame.contains(target.screenPoint) },
            "Start tracking lies under the sheet, so this click cannot reach it.")
        target.click()
        XCTAssertFalse(app.buttons["Stop tracking"].waitForExistence(timeout: 2), "Tracking started behind the sheet.")
        XCTAssertTrue(element("task-name.input").exists, "The click replaced the open sheet.")
        app.buttons["task-name.cancel"].click()
        assertNoDialogRemains()
        XCTAssertFalse(app.buttons["Stop tracking"].waitForExistence(timeout: 2), "Tracking started after the sheet.")

        // The same click starts tracking without a sheet, or the blocked click proves nothing.
        target.click()
        XCTAssertTrue(app.buttons["Stop tracking"].waitForExistence(timeout: timeout))
    }

    func testOpenSheetKeepsTheTrackerWindowOpen() throws {
        launch()
        let window = app.windows.containing(.button, identifier: "bulk-archive.open").firstMatch
        XCTAssertTrue(window.waitForExistence(timeout: timeout))
        openCreation()

        window.buttons[XCUIIdentifierCloseWindow].coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).click()
        XCTAssertFalse(window.waitForNonExistence(timeout: 2), "The close button closed the window behind the sheet.")
        XCTAssertTrue(element("task-name.input").exists, "The close button dismissed the sheet.")

        app.typeKey("w", modifierFlags: .command)
        XCTAssertFalse(window.waitForNonExistence(timeout: 2), "Command-W closed the window behind the sheet.")

        let cancel = app.buttons["task-name.cancel"]
        if cancel.exists { cancel.click() }
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

        let window = app.windows.containing(.button, identifier: "bulk-archive.open").firstMatch
        window.buttons[XCUIIdentifierCloseWindow].coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).click()
        XCTAssertFalse(window.waitForNonExistence(timeout: 2), "The close button closed the window behind the alert.")

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
            // The list's empty area below the rows has its own menu without a task.
            MenuEntry(
                name: "The task list menu",
                target: element("task-sidebar.list").coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.9)),
                open: { $0.rightClick() }, item: "New task"),
            MenuEntry(
                name: "The worklog context menu",
                target: row.coordinate(withNormalizedOffset: CGVector(dx: 0.9, dy: 0.5)),
                open: { $0.rightClick() }, item: "Edit times..."),
            // Its button also turns off while a dialog shows, because no worklog dialog can open then.
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

    /// An editor that opened behind the first dialog would show now. One that never shows would keep the gate claimed
    /// and leave New task disabled.
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
