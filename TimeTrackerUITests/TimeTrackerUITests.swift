import XCTest

final class WorkTaskUITests: XCTestCase {
  var app: XCUIApplication!

  override func setUpWithError() throws {
    continueAfterFailure = false
    app = XCUIApplication()
    app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
    app.launch()
    // On macOS, a sandboxed WindowGroup app does not auto-open a window when
    // launched headlessly by XCUITest. Cmd+N triggers File > New Window.
    app.typeKey("n", modifierFlags: [.command])
  }

  override func tearDownWithError() throws {
    app = nil
  }

  @MainActor
  func test_addTask_isVisibleInListAndHasNoWorkLogs() throws {
    app.buttons["Add Task"].tap()

    // On macOS, Form TextFields have no accessibility label of their own —
    // the label "Title" is a separate StaticText. Query by position instead.
    let sheet = app.sheets.firstMatch
    XCTAssertTrue(sheet.waitForExistence(timeout: 3))

    let titleField = sheet.textFields.firstMatch
    XCTAssertTrue(titleField.waitForExistence(timeout: 3))
    pasteInto(titleField, text: "Buy groceries")

    sheet.buttons["Save"].tap()

    XCTAssertTrue(app.staticTexts["Buy groceries"].waitForExistence(timeout: 3))

    app.staticTexts["Buy groceries"].tap()

    XCTAssertTrue(app.staticTexts["No Work Logs"].waitForExistence(timeout: 3))
  }

  // MARK: - Helpers

  /// Pastes text into an element via the macOS pasteboard.
  /// Using typeText() directly can drop characters on macOS due to timing.
  private func pasteInto(_ element: XCUIElement, text: String) {
    let process = Process()
    process.executableURL = URL(fileURLWithPath: "/bin/sh")
    process.arguments = ["-c", "printf '%s' '\(text)' | pbcopy"]
    try? process.run()
    process.waitUntilExit()
    element.tap()
    element.typeKey("a", modifierFlags: [.command])
    element.typeKey("v", modifierFlags: [.command])
  }
}
