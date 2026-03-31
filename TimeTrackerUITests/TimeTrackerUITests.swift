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

  /// Inputs text into an element in a platform-appropriate way.
  ///
  /// On macOS, `typeText()` can drop characters due to key-event timing, so
  /// we write directly to `NSPasteboard` and paste with Cmd+V instead.
  /// On iOS/iPadOS, `typeText()` is reliable and no pasteboard workaround is needed.
  private func pasteInto(_ element: XCUIElement, text: String) {
    element.tap()
    #if os(macOS)
      NSPasteboard.general.clearContents()
      NSPasteboard.general.setString(text, forType: .string)
      element.typeKey("a", modifierFlags: [.command])
      element.typeKey("v", modifierFlags: [.command])
    #else
      element.typeText(text)
    #endif
  }
}
