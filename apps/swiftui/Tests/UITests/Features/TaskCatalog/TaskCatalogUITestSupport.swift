import XCTest

@MainActor
extension TrackerUITestCase {
    var taskDetailsName: XCUIElement {
        let heading = element("task-details.name")
        let nativeText = heading.staticTexts.firstMatch
        return nativeText.exists ? nativeText : heading
    }

    var taskDetailsNameText: String {
        let text = taskDetailsName
        return text.value as? String ?? text.label
    }
}
