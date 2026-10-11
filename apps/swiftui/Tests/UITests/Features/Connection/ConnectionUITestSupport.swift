import XCTest

@MainActor
extension TrackerUITestCase {
    func openConnectionSettings() {
        app.activate()
        app.typeKey(",", modifierFlags: .command)
        XCTAssertTrue(element("connection.mode").waitForExistence(timeout: timeout))
    }

    func chooseConnection(_ label: String, endpoint: String? = nil) {
        let picker = element("connection.mode")
        picker.click()
        let item = app.menuItems[label].firstMatch
        XCTAssertTrue(item.waitForExistence(timeout: timeout))
        item.click()
        if let endpoint {
            let field = app.textFields["connection.server-url"]
            XCTAssertTrue(field.waitForExistence(timeout: timeout))
            field.click()
            field.typeKey("a", modifierFlags: .command)
            if endpoint.isEmpty { field.typeKey(.delete, modifierFlags: []) }
            else { field.typeText(endpoint) }
        }
    }

    func closeConnectionSettings() {
        let settings = app.windows.containing(.any, identifier: "connection.mode").firstMatch
        settings.buttons[XCUIIdentifierCloseWindow].click()
        waitUntil("Settings closes") { !self.element("connection.mode").exists }
        showTrackerWindow()
    }

    func connectTo(_ endpoint: String?, closeSettings: Bool = true) {
        openConnectionSettings()
        chooseConnection(endpoint == nil ? "Local database" : "Server", endpoint: endpoint)
        app.buttons["connection.connect"].click()
        waitUntil("Connection is adopted") {
            let result = self.element("connection.result")
            return result.exists && self.elementText("connection.result").contains("Connected.")
        }
        if closeSettings { closeConnectionSettings() }
    }

    func assertConnectionStatus(_ text: String) {
        waitUntil("Connection displays \(text)") {
            let status = self.element("connection-summary.status")
            return status.exists && (status.label == text || status.value as? String == text)
        }
    }

    func elementText(_ identifier: String) -> String {
        let target = element(identifier)
        return target.value as? String ?? target.label
    }

    func connectToServer(_ endpoint: String) { connectTo(endpoint, closeSettings: false) }
}

@MainActor
extension TrackerUITestCase {
    func openSettings() {
        app.activate()
        app.typeKey(",", modifierFlags: .command)
        XCTAssertTrue(element("menu.display").waitForExistence(timeout: timeout))
    }
}
