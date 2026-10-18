import XCTest

/// The app prefixes a write failure with this text when both transport attempts went unanswered.
let unconfirmedChangeMessage = "Could not confirm the change. Check the reloaded data before trying again."

@MainActor
extension TrackerUITestCase {
    func displayedText(_ identifier: String) -> String {
        let parent = element(identifier)
        let child = parent.staticTexts.firstMatch
        let text = child.exists ? child : parent
        return text.value as? String ?? text.label
    }

    func waitForText(_ identifier: String, containing text: String,
                     file: StaticString = #filePath, line: UInt = #line) {
        waitUntil("\(identifier) shows \(text)", file: file, line: line) {
            self.element(identifier).exists && self.displayedText(identifier).localizedCaseInsensitiveContains(text)
        }
    }
}
