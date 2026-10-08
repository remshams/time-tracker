import XCTest

@MainActor
extension TrackerUITestCase {
    func assertAcceptedWriteIntent(_ proxy: ControlledProxy, method: String, path: String,
                                   file: StaticString = #filePath, line: UInt = #line) throws {
        let attempts = try proxy.requests(method: method, path: path)
        XCTAssertEqual(attempts.count, 2, "The initial write and its transport retry are the only attempts.",
                       file: file, line: line)
        let first = try XCTUnwrap(attempts.first, file: file, line: line)
        let body = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(first.body.utf8)) as? [String: Any],
                                 file: file, line: line)
        let requestID = try XCTUnwrap(body["request_id"] as? String, file: file, line: line)
        XCTAssertNotNil(UUID(uuidString: requestID), "Each accepted intent has a request ID.", file: file, line: line)
        for attempt in attempts {
            XCTAssertEqual(attempt.body, first.body, "Transport retries preserve the entire write intent.",
                           file: file, line: line)
            let status = try XCTUnwrap(attempt.status, file: file, line: line)
            XCTAssertTrue((200..<300).contains(status), "The server accepted each idempotent attempt.",
                          file: file, line: line)
        }
    }

    var taskDetailsName: XCUIElement {
        let heading = element("task-details.name")
        let nativeText = heading.staticTexts.firstMatch
        return nativeText.exists ? nativeText : heading
    }

    var taskDetailsNameText: String {
        let text = taskDetailsName
        return text.value as? String ?? text.label
    }

    func displayedText(_ identifier: String) -> String {
        let parent = element(identifier)
        let child = parent.staticTexts.firstMatch
        let text = child.exists ? child : parent
        return text.value as? String ?? text.label
    }

    func openCreation() {
        let button = app.buttons["New task"]
        waitUntil("Creation becomes available") { button.exists && button.isEnabled }
        button.click()
        XCTAssertTrue(element("task-name.input").waitForExistence(timeout: timeout))
    }

    func openRename(_ task: FixtureTask, contextMenu: Bool = false) {
        if contextMenu {
            taskRow(task).rightClick()
            app.menuItems["Edit name"].click()
        } else {
            select(task)
            app.buttons["Edit task name"].click()
        }
        XCTAssertTrue(element("task-name.input").waitForExistence(timeout: timeout))
    }

    func openWorklogAction(_ log: FixtureWorklog, title: String, sheetID: String) {
        let actions = element("worklog-history.actions.\(log.id)")
        XCTAssertTrue(actions.waitForExistence(timeout: timeout))
        waitUntil("Worklog actions become available") { actions.isEnabled }
        actions.click()
        app.menuItems[title].click()
        XCTAssertTrue(element(sheetID).waitForExistence(timeout: timeout))
    }

    func openCorrection(_ log: FixtureWorklog) {
        openWorklogAction(log, title: "Edit times...", sheetID: "worklog-correction.start")
    }

    func openMove(_ log: FixtureWorklog) {
        openWorklogAction(log, title: "Move to task...", sheetID: "worklog-move.search")
    }

    func stepMinute(_ identifier: String, direction: XCUIKeyboardKey = .upArrow) {
        let picker = element(identifier)
        // The en_US picker exposes one element; its minute segment precedes AM/PM and the stepper.
        picker.coordinate(withNormalizedOffset: CGVector(dx: 0.70, dy: 0.5)).click()
        picker.typeKey(direction, modifierFlags: [])
        // Move past AM/PM to commit the edited segment without submitting the sheet.
        app.typeKey(.tab, modifierFlags: [])
        app.typeKey(.tab, modifierFlags: [])
    }
}
