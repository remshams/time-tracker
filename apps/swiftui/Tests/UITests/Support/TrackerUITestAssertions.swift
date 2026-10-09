import XCTest

@MainActor
extension TrackerUITestCase {
    func displayedText(_ identifier: String) -> String {
        let parent = element(identifier)
        let child = parent.staticTexts.firstMatch
        let text = child.exists ? child : parent
        return text.value as? String ?? text.label
    }
}
