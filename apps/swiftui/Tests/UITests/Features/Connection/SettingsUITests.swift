import XCTest

@MainActor
final class SettingsUITests: TrackerUITestCase {
    func testMenuTotalAndAutomaticPausePreferencesApplyAndPersist() throws {
        _ = try fixture.create("Settings task")
        launch()
        openSettings()
        let total = element("menu.show-daily-total")
        let pause = element("tracking.pause-on-lock")
        XCTAssertEqual((total.value as? NSNumber)?.boolValue, true)
        XCTAssertEqual((pause.value as? NSNumber)?.boolValue, false)
        total.click()
        pause.click()
        waitUntil("The status item removes its total immediately") { self.statusButton.value as? String == "" }
        relaunch()
        openSettings()
        XCTAssertEqual((element("menu.show-daily-total").value as? NSNumber)?.boolValue, false)
        XCTAssertEqual((element("tracking.pause-on-lock").value as? NSNumber)?.boolValue, true)
        element("menu.show-daily-total").click()
        waitUntil("The status item restores its total immediately") { self.statusButton.value as? String != "" }
    }

    func testShortcutRecordingCancelSaveRelaunchAndRestore() throws {
        launch()
        openSettings()
        let recorder = element("global-shortcut.open-menu")
        XCTAssertEqual(recorder.value as? String, "⌃⌥T")
        recorder.click()
        XCTAssertEqual(recorder.value as? String, "Press shortcut...")
        app.typeKey(.escape, modifierFlags: [])
        XCTAssertEqual(recorder.value as? String, "⌃⌥T")
        recorder.click()
        app.typeKey("y", modifierFlags: [.control, .option])
        waitUntil("The shortcut records its new keys") { recorder.value as? String == "⌃⌥Y" }
        relaunch()
        app.typeKey("y", modifierFlags: [.control, .option])
        XCTAssertTrue(app.menuItems["Open Time Tracker"].waitForExistence(timeout: timeout))
        dismissStatusMenu()
        openSettings()
        XCTAssertEqual(element("global-shortcut.open-menu").value as? String, "⌃⌥Y")
        element("global-shortcut.restore").click()
        XCTAssertEqual(element("global-shortcut.open-menu").value as? String, "⌃⌥T")
        openStatusMenu()
        dismissStatusMenu()
    }

    func testShortcutRegistrationConflictKeepsPreviousBinding() throws {
        app.launchEnvironment["TT_UI_TEST_CONFLICT_KEY"] = "y"
        launch()
        openSettings()
        element("global-shortcut.open-menu").click()
        app.typeKey("y", modifierFlags: [.control, .option])
        XCTAssertTrue(element("global-shortcut.error").waitForExistence(timeout: timeout))
        XCTAssertEqual(element("global-shortcut.open-menu").value as? String, "⌃⌥T")
        openStatusMenu()
        dismissStatusMenu()
    }

    func testQuickTrackRegistrationErrorSurvivesTheMenuAndOtherChanges() throws {
        // Every registration of a Space shortcut fails, including the Quick Track default at launch.
        app.launchEnvironment["TT_UI_TEST_CONFLICT_KEY"] = " "
        launch()
        openSettings()
        let error = element("global-shortcut.error")
        XCTAssertTrue(error.waitForExistence(timeout: timeout), "The failed Quick Track hotkey shows an error.")
        openStatusMenu()
        dismissStatusMenu()
        openSettings()
        XCTAssertTrue(error.waitForExistence(timeout: timeout), "Closing the menu keeps the Quick Track error.")
        let openMenu = element("global-shortcut.open-menu")
        openMenu.click()
        app.typeKey("y", modifierFlags: [.control, .option])
        waitUntil("Open menu records its new keys") { openMenu.value as? String == "⌃⌥Y" }
        XCTAssertTrue(error.exists, "The Quick Track error stays while its hotkey still fails.")
        element("global-shortcut.restore").click()
        waitUntil("Restoring the defaults resets Open menu") { openMenu.value as? String == "⌃⌥T" }
    }

    func testQuickTrackShortcutRecordsSpaceAndRestoresItsDefault() throws {
        launch()
        openSettings()
        let recorder = element("global-shortcut.quick-track")
        XCTAssertEqual(recorder.value as? String, "⌃⌥Space")
        XCTAssertEqual(element("global-shortcut.open-menu").value as? String, "⌃⌥T")
        recorder.click()
        app.typeKey("q", modifierFlags: [.control, .option])
        waitUntil("The Quick Track shortcut records its new keys") { recorder.value as? String == "⌃⌥Q" }
        recorder.click()
        // ⌃⌥⇧Space has no system binding; ⌘⌥Space opens a Finder search window.
        app.typeKey(" ", modifierFlags: [.control, .option, .shift])
        waitUntil("The Quick Track shortcut records Space") { recorder.value as? String == "⌃⌥⇧Space" }
        XCTAssertEqual(element("global-shortcut.open-menu").value as? String, "⌃⌥T")
        relaunch()
        openSettings()
        XCTAssertEqual(element("global-shortcut.quick-track").value as? String, "⌃⌥⇧Space")
        element("global-shortcut.restore").click()
        XCTAssertEqual(element("global-shortcut.quick-track").value as? String, "⌃⌥Space")
        XCTAssertEqual(element("global-shortcut.open-menu").value as? String, "⌃⌥T")
        openStatusMenu()
        dismissStatusMenu()
    }

    func testQuickTrackShortcutRejectsTheOpenMenuBinding() throws {
        launch()
        openSettings()
        element("global-shortcut.quick-track").click()
        app.typeKey("t", modifierFlags: [.control, .option])
        let error = element("global-shortcut.error")
        XCTAssertTrue(error.waitForExistence(timeout: timeout))
        XCTAssertTrue(
            (error.value as? String ?? error.label).contains("Open Quick Track and Open menu use the same shortcut"))
        XCTAssertEqual(element("global-shortcut.quick-track").value as? String, "⌃⌥Space")
        XCTAssertEqual(element("global-shortcut.open-menu").value as? String, "⌃⌥T")
    }
}
