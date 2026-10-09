import XCTest

@MainActor
final class SettingsUITests: TrackerUITestCase {
    func testMenuBarDisplayAndAutomaticPausePreferencesApplyAndPersist() throws {
        _ = try fixture.create("Settings task")
        launch()
        openSettings()
        let display = element("menu.display")
        let pause = element("tracking.pause-on-lock")
        XCTAssertEqual(display.value as? String, "Time")
        XCTAssertEqual((pause.value as? NSNumber)?.boolValue, false)
        chooseMenuBarDisplay("None")
        pause.click()
        waitUntil("The status item removes its total immediately") { self.statusButton.value as? String == "" }
        relaunch()
        openSettings()
        XCTAssertEqual(element("menu.display").value as? String, "None")
        XCTAssertEqual((element("tracking.pause-on-lock").value as? NSNumber)?.boolValue, true)
        chooseMenuBarDisplay("Time")
        waitUntil("The status item restores its total immediately") { self.statusButton.value as? String != "" }
    }

    func testShortcutRecordingCancelSaveRelaunchAndRestore() throws {
        launch()
        openSettings()
        let recorder = element("menu.shortcut.open")
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
        XCTAssertEqual(element("menu.shortcut.open").value as? String, "⌃⌥Y")
        element("menu.shortcut.restore").click()
        XCTAssertEqual(element("menu.shortcut.open").value as? String, "⌃⌥T")
        openStatusMenu()
        dismissStatusMenu()
    }

    func testShortcutRegistrationConflictKeepsPreviousBinding() throws {
        app.launchEnvironment["TT_UI_TEST_CONFLICT_KEY"] = "y"
        launch()
        openSettings()
        element("menu.shortcut.open").click()
        app.typeKey("y", modifierFlags: [.control, .option])
        XCTAssertTrue(element("menu.shortcut.error").waitForExistence(timeout: timeout))
        XCTAssertEqual(element("menu.shortcut.open").value as? String, "⌃⌥T")
        openStatusMenu()
        dismissStatusMenu()
    }
}
