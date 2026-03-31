import XCTest
#if os(macOS)
  import AppKit
#endif

final class WorkTaskUITests: XCTestCase {
  private enum Constants {
    static let addTaskButtonIdentifier = "add-task-button"
    static let taskListReadyIdentifier = "task-list-ready"
    static let createdTaskTitle = "Buy groceries"
    static let uiTimeout: TimeInterval = 5
  }

  var app: XCUIApplication!

  override func setUpWithError() throws {
    continueAfterFailure = false
    app = XCUIApplication()
    app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
    app.launch()
    ensureMainWindowIsOpen()
  }

  override func tearDownWithError() throws {
    app = nil
  }

  @MainActor
  func test_addTask_isVisibleInListAndHasNoWorkLogs() throws {
    let addTaskButton = app.buttons[Constants.addTaskButtonIdentifier]
    XCTAssertTrue(addTaskButton.waitForExistence(timeout: Constants.uiTimeout))
    addTaskButton.tap()

    // On macOS, Form TextFields have no accessibility label of their own —
    // the label "Title" is a separate StaticText. Query by position instead.
    let sheet = app.sheets.firstMatch
    XCTAssertTrue(sheet.waitForExistence(timeout: Constants.uiTimeout))

    let titleField = sheet.textFields.firstMatch
    XCTAssertTrue(titleField.waitForExistence(timeout: Constants.uiTimeout))
    pasteInto(titleField, text: Constants.createdTaskTitle)

    sheet.buttons["Save"].tap()

    XCTAssertTrue(app.staticTexts[Constants.createdTaskTitle].waitForExistence(timeout: Constants.uiTimeout))

    app.staticTexts[Constants.createdTaskTitle].tap()

    XCTAssertTrue(app.staticTexts["No Work Logs"].waitForExistence(timeout: Constants.uiTimeout))
  }

  // MARK: - Helpers

  /// Ensures the main app window is ready before test interactions begin.
  ///
  /// On macOS, a sandboxed `WindowGroup` app launched headlessly by XCUITest
  /// may start without an open window. Trigger `File > New Window` and wait
  /// for a stable accessible marker in the task list before continuing.
  private func ensureMainWindowIsOpen() {
    #if os(macOS)
      let readyMarker = app.staticTexts[Constants.taskListReadyIdentifier]
      if readyMarker.waitForExistence(timeout: Constants.uiTimeout) {
        return
      }

      app.typeKey("n", modifierFlags: [.command])
      XCTAssertTrue(readyMarker.waitForExistence(timeout: Constants.uiTimeout))
    #endif
  }

  /// Inputs text into an element in a platform-appropriate way.
  ///
  /// On macOS, `typeText()` can drop characters due to key-event timing, so
  /// we write directly to `NSPasteboard` and paste with Cmd+V instead.
  /// Do not use this helper with sensitive values because it writes through the
  /// global pasteboard.
  /// On iOS/iPadOS, `typeText()` is reliable and no pasteboard workaround is needed.
  private func pasteInto(_ element: XCUIElement, text: String) {
    element.tap()
    #if os(macOS)
      let pasteboard = NSPasteboard.general
      let previousString = pasteboard.string(forType: .string)
      defer {
        pasteboard.clearContents()
        if let previousString {
          pasteboard.setString(previousString, forType: .string)
        }
      }

      pasteboard.clearContents()
      pasteboard.setString(text, forType: .string)
      element.typeKey("a", modifierFlags: [.command])
      element.typeKey("v", modifierFlags: [.command])
    #else
      element.typeText(text)
    #endif
  }
}
